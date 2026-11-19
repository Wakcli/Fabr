using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Text;

namespace Fabr.Bindings;

public static unsafe class ScriptHost
{
    private sealed class Loaded
    {
        public required FabrScript Instance;
        public required AssemblyLoadContext Context;
    }

    private static readonly Dictionary<ulong, Loaded> Scripts = new();

    private sealed class ScriptContext(string name) : AssemblyLoadContext(name, isCollectible: true)
    {
        protected override Assembly? Load(AssemblyName name)
        {
            Assembly bindings = typeof(FabrScript).Assembly;
            return name.Name == bindings.GetName().Name ? bindings : null;
        }
    }

    [UnmanagedCallersOnly]
    public static int Load(FabrApi* api, ulong handle, byte* assemblyPath, byte* typeName)
    {
        if (api->Version < FabrOps.AbiVersion || api->Size < (uint)sizeof(FabrApi))
        {
            Console.Error.WriteLine(
                $"Fabr.Bindings needs engine ABI {FabrOps.AbiVersion} and a {sizeof(FabrApi)}-byte table, " +
                $"engine provides {api->Version} and {api->Size} bytes");
            return FabrOps.AbiMismatch;
        }
        try
        {
            string path = Marshal.PtrToStringUTF8((IntPtr)assemblyPath) ?? throw new ArgumentException("missing assembly path");
            string type = Marshal.PtrToStringUTF8((IntPtr)typeName) ?? throw new ArgumentException("missing class name");
            Release(handle);
            AssemblyLoadContext context = new ScriptContext($"fabr-{handle}");
            // Path loads are cached for the life of the process: reloading the same
            // path after a rebuild would hand back the previous assembly image.
            byte[] image = File.ReadAllBytes(Path.GetFullPath(path));
            Assembly assembly = context.LoadFromStream(new MemoryStream(image));
            Type? script = assembly.GetType(type, throwOnError: true);
            if (script is null || !typeof(FabrScript).IsAssignableFrom(script))
            {
                context.Unload();
                throw new InvalidOperationException($"{type} must derive from FabrScript");
            }
            FabrScript instance = (FabrScript)Activator.CreateInstance(script)!;
            instance.Api = api;
            instance.Self = handle;
            Scripts[handle] = new Loaded { Instance = instance, Context = context };
            return 0;
        }
        catch (Exception error)
        {
            Report(api, "load", error);
            return 1;
        }
    }

    [UnmanagedCallersOnly]
    public static int Start(FabrApi* api, ulong handle)
    {
        try
        {
            if (!Scripts.TryGetValue(handle, out Loaded? loaded))
            {
                throw new InvalidOperationException($"script {handle} is not loaded");
            }
            loaded.Instance.Api = api;
            loaded.Instance.OnStart();
            return 0;
        }
        catch (Exception error)
        {
            Report(api, "start", error);
            return 1;
        }
    }

    [UnmanagedCallersOnly]
    public static int Update(FabrApi* api, ulong handle, float dt)
    {
        try
        {
            if (!Scripts.TryGetValue(handle, out Loaded? loaded))
            {
                throw new InvalidOperationException($"script {handle} is not loaded");
            }
            loaded.Instance.Api = api;
            loaded.Instance.OnUpdate(dt);
            return 0;
        }
        catch (Exception error)
        {
            Report(api, "update", error);
            return 1;
        }
    }

    [UnmanagedCallersOnly]
    public static int Stop(FabrApi* api, ulong handle)
    {
        try
        {
            if (!Scripts.TryGetValue(handle, out Loaded? loaded))
            {
                return 0;
            }
            loaded.Instance.Api = api;
            loaded.Instance.OnStop();
            return 0;
        }
        catch (Exception error)
        {
            Report(api, "stop", error);
            return 1;
        }
    }

    [UnmanagedCallersOnly]
    public static int Unload(ulong handle)
    {
        Release(handle);
        return 0;
    }

    [UnmanagedCallersOnly]
    public static int ApplyProperties(FabrApi* api, ulong handle, byte* payload)
    {
        try
        {
            if (!Scripts.TryGetValue(handle, out Loaded? loaded))
            {
                throw new InvalidOperationException($"script {handle} is not loaded");
            }
            loaded.Instance.Api = api;
            string text = Marshal.PtrToStringUTF8((IntPtr)payload) ?? string.Empty;
            ScriptProperties.Apply(loaded.Instance, text);
            return 0;
        }
        catch (Exception error)
        {
            FabrOps.Log(api, $"csharp properties: {error.GetType().Name}: {error.Message}");
            return 1;
        }
    }

    // Reads the declared properties without starting the script: the assembly
    // goes into a throwaway load context, the class is constructed so field
    // initializers run, and the context is unloaded before returning.
    [UnmanagedCallersOnly]
    public static int Inspect(FabrApi* api, byte* assemblyPath, byte* typeName, byte* buffer, uint length)
    {
        AssemblyLoadContext? context = null;
        try
        {
            string path = Marshal.PtrToStringUTF8((IntPtr)assemblyPath) ?? throw new ArgumentException("missing assembly path");
            string type = Marshal.PtrToStringUTF8((IntPtr)typeName) ?? throw new ArgumentException("missing class name");
            context = new ScriptContext("fabr-inspect");
            byte[] image = File.ReadAllBytes(Path.GetFullPath(path));
            Assembly assembly = context.LoadFromStream(new MemoryStream(image));
            Type? script = assembly.GetType(type, throwOnError: true);
            if (script is null || !typeof(FabrScript).IsAssignableFrom(script))
            {
                throw new InvalidOperationException($"{type} must derive from FabrScript");
            }
            FabrScript instance = (FabrScript)Activator.CreateInstance(script)!;
            instance.Api = api;
            return Write(ScriptProperties.Encode(instance), buffer, length);
        }
        catch (Exception error)
        {
            FabrOps.Log(api, $"csharp inspect: {error.GetType().Name}: {error.Message}");
            return -1;
        }
        finally
        {
            context?.Unload();
        }
    }

    private static int Write(string text, byte* buffer, uint length)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(text);
        if (buffer is null || length < (uint)bytes.Length)
        {
            return bytes.Length;
        }
        Marshal.Copy(bytes, 0, (IntPtr)buffer, bytes.Length);
        return bytes.Length;
    }

    private static void Release(ulong handle)
    {
        if (Scripts.Remove(handle, out Loaded? loaded))
        {
            loaded.Context.Unload();
        }
    }

    private static void Report(FabrApi* api, string phase, Exception error)
    {
        FabrOps.Log(api, $"csharp {phase}: {error.GetType().Name}: {error.Message}");
    }
}

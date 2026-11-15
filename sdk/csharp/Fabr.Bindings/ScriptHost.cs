using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.Loader;

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
        if (api->Version < Fabr.AbiVersion || api->Size < (uint)sizeof(FabrApi))
        {
            Console.Error.WriteLine(
                $"Fabr.Bindings needs engine ABI {Fabr.AbiVersion} and a {sizeof(FabrApi)}-byte table, " +
                $"engine provides {api->Version} and {api->Size} bytes");
            return Fabr.AbiMismatch;
        }
        try
        {
            string path = Marshal.PtrToStringUTF8((IntPtr)assemblyPath) ?? throw new ArgumentException("missing assembly path");
            string type = Marshal.PtrToStringUTF8((IntPtr)typeName) ?? throw new ArgumentException("missing class name");
            Release(handle);
            AssemblyLoadContext context = new ScriptContext($"fabr-{handle}");
            Assembly assembly = context.LoadFromAssemblyPath(Path.GetFullPath(path));
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

    private static void Release(ulong handle)
    {
        if (Scripts.Remove(handle, out Loaded? loaded))
        {
            loaded.Context.Unload();
        }
    }

    private static void Report(FabrApi* api, string phase, Exception error)
    {
        Fabr.Log(api, $"csharp {phase}: {error.GetType().Name}: {error.Message}");
    }
}

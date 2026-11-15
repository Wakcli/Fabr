using System.Numerics;
using System.Runtime.InteropServices;
using System.Text;

namespace Fabr.Bindings;

[StructLayout(LayoutKind.Sequential)]
public unsafe struct FabrApi
{
    public uint Version;
    public uint Size;
    public void* Context;
    public delegate* unmanaged[Cdecl]<void*, byte*, void> Log;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, int> SetTranslation;
    public delegate* unmanaged[Cdecl]<void*, ulong, float*, int> GetTranslation;
}

public static unsafe class Fabr
{
    public const uint AbiVersion = 1;

    public const int AbiMismatch = 2;

    public static void Log(FabrApi* api, string message)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(message + "\0");
        fixed (byte* text = bytes)
        {
            api->Log(api->Context, text);
        }
    }

    public static bool TryGetPosition(FabrApi* api, ulong entity, out Vector3 position)
    {
        float* values = stackalloc float[3];
        if (api->GetTranslation(api->Context, entity, values) == 0)
        {
            position = default;
            return false;
        }
        position = new Vector3(values[0], values[1], values[2]);
        return true;
    }

    public static Vector3 GetPosition(FabrApi* api, ulong entity) =>
        TryGetPosition(api, entity, out Vector3 position) ? position : default;

    public static bool SetPosition(FabrApi* api, ulong entity, Vector3 position) =>
        api->SetTranslation(api->Context, entity, position.X, position.Y, position.Z) != 0;

    public static bool Translate(FabrApi* api, ulong entity, Vector3 offset)
    {
        if (!TryGetPosition(api, entity, out Vector3 position))
        {
            return false;
        }
        return SetPosition(api, entity, position + offset);
    }
}

public abstract unsafe class FabrScript
{
    public ulong Self { get; internal set; }

    internal FabrApi* Api { get; set; }

    public Vector3 Position => Fabr.GetPosition(Api, Self);

    public bool TryGetPosition(out Vector3 position) => Fabr.TryGetPosition(Api, Self, out position);

    public void SetPosition(Vector3 position) => Fabr.SetPosition(Api, Self, position);

    public void Translate(Vector3 offset) => Fabr.Translate(Api, Self, offset);

    public void Log(string message) => Fabr.Log(Api, message);

    public virtual void OnStart()
    {
    }

    public virtual void OnUpdate(float dt)
    {
    }

    public virtual void OnStop()
    {
    }
}

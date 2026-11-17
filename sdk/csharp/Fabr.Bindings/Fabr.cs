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
    public delegate* unmanaged[Cdecl]<void*, ulong, float*, int> Translation;
    public delegate* unmanaged[Cdecl]<void*, float> Delta;
    public delegate* unmanaged[Cdecl]<void*, float> Elapsed;
    public delegate* unmanaged[Cdecl]<void*, byte*, int> Key;
    public delegate* unmanaged[Cdecl]<void*, byte*, int> KeyDown;
    public delegate* unmanaged[Cdecl]<void*, byte, int> Mouse;
    public delegate* unmanaged[Cdecl]<void*, byte*, int> Action;
    public delegate* unmanaged[Cdecl]<void*, byte*, int> ActionDown;
    public delegate* unmanaged[Cdecl]<void*, byte*, ulong*, int> Find;
    public delegate* unmanaged[Cdecl]<void*, ulong, byte*, uint, int> Name;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, int> Translate;
    public delegate* unmanaged[Cdecl]<void*, ulong, float*, int> Rotation;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, float, int> SetRotation;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, int> RotateY;
    public delegate* unmanaged[Cdecl]<void*, ulong, float*, int> Velocity;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, int> SetVelocity;
    public delegate* unmanaged[Cdecl]<void*, ulong, int> Grounded;
    public delegate* unmanaged[Cdecl]<void*, ulong, float*, int> AngularVelocity;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, int> SetAngularVelocity;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, int> AddSpin;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, int> Walk;
    public delegate* unmanaged[Cdecl]<void*, ulong, int> Jump;
    public delegate* unmanaged[Cdecl]<void*, ulong, uint> AnimationClipCount;
    public delegate* unmanaged[Cdecl]<void*, ulong, uint, byte*, uint, int> AnimationClip;
    public delegate* unmanaged[Cdecl]<void*, ulong, byte*, int> PlayAnimation;
    public delegate* unmanaged[Cdecl]<void*, ulong, int> StopAnimation;
    public delegate* unmanaged[Cdecl]<void*, float*, float*, float*, uint, uint> FindPath;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, float, int> MoveTo;
    public delegate* unmanaged[Cdecl]<void*, ulong, int> StopMoving;
    public delegate* unmanaged[Cdecl]<void*, byte*, float, int, int> PlaySound;
    public delegate* unmanaged[Cdecl]<void*, void> UiClear;
    public delegate* unmanaged[Cdecl]<void*, float, float, float, byte*, float, float, float, void> UiText;
    public delegate* unmanaged[Cdecl]<void*, float*, float*, void> UiPanel;
    public delegate* unmanaged[Cdecl]<void*, uint, float*, byte*, void> UiButton;
    public delegate* unmanaged[Cdecl]<void*, uint, int> UiClicked;
    public delegate* unmanaged[Cdecl]<void*, byte*, int> LoadScene;
    public delegate* unmanaged[Cdecl]<void*, int> ReloadScene;
    public delegate* unmanaged[Cdecl]<void*, byte*, ulong*, int> LoadSceneAdditive;
    public delegate* unmanaged[Cdecl]<void*, ulong, int> UnloadScene;
    public delegate* unmanaged[Cdecl]<void*, ulong, ulong, float, float, float, int> CreateJoint;
}

public enum MouseButton : byte
{
    Left = 1,
    Right = 2,
    Middle = 3,
}

public enum AudioBus
{
    Sfx = 0,
    Music = 1,
}

public static unsafe class FabrOps
{
    public const uint AbiVersion = 2;

    public const int AbiMismatch = 2;

    private const int StringBuffer = 256;

    private static byte[] Utf8(string text)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(text);
        Array.Resize(ref bytes, bytes.Length + 1);
        return bytes;
    }

    public static void Log(FabrApi* api, string message)
    {
        byte[] bytes = Utf8(message);
        fixed (byte* text = bytes)
        {
            api->Log(api->Context, text);
        }
    }

    public static float Delta(FabrApi* api) => api->Delta(api->Context);

    public static float Elapsed(FabrApi* api) => api->Elapsed(api->Context);

    public static bool Key(FabrApi* api, string name)
    {
        byte[] bytes = Utf8(name);
        fixed (byte* text = bytes)
        {
            return api->Key(api->Context, text) != 0;
        }
    }

    public static bool KeyDown(FabrApi* api, string name)
    {
        byte[] bytes = Utf8(name);
        fixed (byte* text = bytes)
        {
            return api->KeyDown(api->Context, text) != 0;
        }
    }

    public static bool Mouse(FabrApi* api, MouseButton button) =>
        api->Mouse(api->Context, (byte)button) != 0;

    public static bool Action(FabrApi* api, string name)
    {
        byte[] bytes = Utf8(name);
        fixed (byte* text = bytes)
        {
            return api->Action(api->Context, text) != 0;
        }
    }

    public static bool ActionDown(FabrApi* api, string name)
    {
        byte[] bytes = Utf8(name);
        fixed (byte* text = bytes)
        {
            return api->ActionDown(api->Context, text) != 0;
        }
    }

    public static ulong? Find(FabrApi* api, string name)
    {
        byte[] bytes = Utf8(name);
        ulong handle;
        fixed (byte* text = bytes)
        {
            if (api->Find(api->Context, text, &handle) == 0)
            {
                return null;
            }
        }
        return handle;
    }

    public static string? Name(FabrApi* api, ulong entity)
    {
        Span<byte> buffer = stackalloc byte[StringBuffer];
        while (true)
        {
            int len;
            fixed (byte* text = buffer)
            {
                len = api->Name(api->Context, entity, text, (uint)buffer.Length);
            }
            if (len < 0)
            {
                return null;
            }
            if (len < buffer.Length)
            {
                return Encoding.UTF8.GetString(buffer[..len]);
            }
            buffer = new byte[buffer.Length * 2];
        }
    }

    public static bool TryGetTranslation(FabrApi* api, ulong entity, out Vector3 translation)
    {
        float* values = stackalloc float[3];
        if (api->Translation(api->Context, entity, values) == 0)
        {
            translation = default;
            return false;
        }
        translation = new Vector3(values[0], values[1], values[2]);
        return true;
    }

    public static Vector3 GetTranslation(FabrApi* api, ulong entity) =>
        TryGetTranslation(api, entity, out Vector3 translation) ? translation : default;

    public static bool SetTranslation(FabrApi* api, ulong entity, Vector3 translation) =>
        api->SetTranslation(api->Context, entity, translation.X, translation.Y, translation.Z) != 0;

    public static bool Translate(FabrApi* api, ulong entity, Vector3 offset) =>
        api->Translate(api->Context, entity, offset.X, offset.Y, offset.Z) != 0;

    public static bool TryGetRotation(FabrApi* api, ulong entity, out Quaternion rotation)
    {
        float* values = stackalloc float[4];
        if (api->Rotation(api->Context, entity, values) == 0)
        {
            rotation = default;
            return false;
        }
        rotation = new Quaternion(values[0], values[1], values[2], values[3]);
        return true;
    }

    public static Quaternion GetRotation(FabrApi* api, ulong entity) =>
        TryGetRotation(api, entity, out Quaternion rotation) ? rotation : default;

    public static bool SetRotation(FabrApi* api, ulong entity, Quaternion rotation) =>
        api->SetRotation(api->Context, entity, rotation.X, rotation.Y, rotation.Z, rotation.W) != 0;

    public static bool RotateY(FabrApi* api, ulong entity, float radians) =>
        api->RotateY(api->Context, entity, radians) != 0;

    public static bool TryGetVelocity(FabrApi* api, ulong entity, out Vector3 velocity)
    {
        float* values = stackalloc float[3];
        if (api->Velocity(api->Context, entity, values) == 0)
        {
            velocity = default;
            return false;
        }
        velocity = new Vector3(values[0], values[1], values[2]);
        return true;
    }

    public static Vector3 GetVelocity(FabrApi* api, ulong entity) =>
        TryGetVelocity(api, entity, out Vector3 velocity) ? velocity : default;

    public static bool SetVelocity(FabrApi* api, ulong entity, Vector3 velocity) =>
        api->SetVelocity(api->Context, entity, velocity.X, velocity.Y, velocity.Z) != 0;

    public static bool Grounded(FabrApi* api, ulong entity) =>
        api->Grounded(api->Context, entity) != 0;

    public static bool TryGetAngularVelocity(FabrApi* api, ulong entity, out Vector3 angular)
    {
        float* values = stackalloc float[3];
        if (api->AngularVelocity(api->Context, entity, values) == 0)
        {
            angular = default;
            return false;
        }
        angular = new Vector3(values[0], values[1], values[2]);
        return true;
    }

    public static Vector3 GetAngularVelocity(FabrApi* api, ulong entity) =>
        TryGetAngularVelocity(api, entity, out Vector3 angular) ? angular : default;

    public static bool SetAngularVelocity(FabrApi* api, ulong entity, Vector3 angular) =>
        api->SetAngularVelocity(api->Context, entity, angular.X, angular.Y, angular.Z) != 0;

    public static bool AddSpin(FabrApi* api, ulong entity, Vector3 impulse) =>
        api->AddSpin(api->Context, entity, impulse.X, impulse.Y, impulse.Z) != 0;

    public static bool Walk(FabrApi* api, ulong entity, float x, float z) =>
        api->Walk(api->Context, entity, x, z) != 0;

    public static bool Jump(FabrApi* api, ulong entity) => api->Jump(api->Context, entity) != 0;

    public static string[] AnimationClips(FabrApi* api, ulong entity)
    {
        uint count = api->AnimationClipCount(api->Context, entity);
        if (count == 0)
        {
            return Array.Empty<string>();
        }
        string[] clips = new string[count];
        byte[] buffer = new byte[StringBuffer];
        for (uint i = 0; i < count; i++)
        {
            if (buffer.Length != StringBuffer)
            {
                buffer = new byte[StringBuffer];
            }
            while (true)
            {
                int len;
                fixed (byte* text = buffer)
                {
                    len = api->AnimationClip(api->Context, entity, i, text, (uint)buffer.Length);
                }
                if (len < 0)
                {
                    clips[i] = string.Empty;
                    break;
                }
                if (len < buffer.Length)
                {
                    clips[i] = Encoding.UTF8.GetString(buffer, 0, len);
                    break;
                }
                buffer = new byte[buffer.Length * 2];
            }
        }
        return clips;
    }

    public static bool PlayAnimation(FabrApi* api, ulong entity, string name)
    {
        byte[] bytes = Utf8(name);
        fixed (byte* text = bytes)
        {
            return api->PlayAnimation(api->Context, entity, text) != 0;
        }
    }

    public static bool StopAnimation(FabrApi* api, ulong entity) =>
        api->StopAnimation(api->Context, entity) != 0;

    public static Vector3[] FindPath(FabrApi* api, Vector3 from, Vector3 to)
    {
        float* start = stackalloc float[3] { from.X, from.Y, from.Z };
        float* goal = stackalloc float[3] { to.X, to.Y, to.Z };
        uint count = api->FindPath(api->Context, start, goal, null, 0);
        if (count == 0)
        {
            return Array.Empty<Vector3>();
        }
        float[] points = new float[count * 3];
        fixed (float* outPoints = points)
        {
            api->FindPath(api->Context, start, goal, outPoints, count);
        }
        Vector3[] path = new Vector3[count];
        for (int i = 0; i < count; i++)
        {
            path[i] = new Vector3(points[i * 3], points[i * 3 + 1], points[i * 3 + 2]);
        }
        return path;
    }

    public static bool MoveTo(FabrApi* api, ulong entity, Vector3 goal, float speed) =>
        api->MoveTo(api->Context, entity, goal.X, goal.Y, goal.Z, speed) != 0;

    public static bool StopMoving(FabrApi* api, ulong entity) =>
        api->StopMoving(api->Context, entity) != 0;

    public static bool PlaySound(FabrApi* api, string path, float volume, AudioBus bus)
    {
        byte[] bytes = Utf8(path);
        fixed (byte* text = bytes)
        {
            return api->PlaySound(api->Context, text, volume, (int)bus) != 0;
        }
    }

    public static void UiClear(FabrApi* api) => api->UiClear(api->Context);

    public static void UiText(FabrApi* api, float x, float y, float size, string text, Vector3 color)
    {
        byte[] bytes = Utf8(text);
        fixed (byte* label = bytes)
        {
            api->UiText(api->Context, x, y, size, label, color.X, color.Y, color.Z);
        }
    }

    public static void UiPanel(FabrApi* api, float x, float y, float w, float h, Vector4 color)
    {
        float* rect = stackalloc float[4] { x, y, w, h };
        float* fill = stackalloc float[4] { color.X, color.Y, color.Z, color.W };
        api->UiPanel(api->Context, rect, fill);
    }

    public static void UiButton(FabrApi* api, uint id, float x, float y, float w, float h, string label)
    {
        byte[] bytes = Utf8(label);
        float* rect = stackalloc float[4] { x, y, w, h };
        fixed (byte* text = bytes)
        {
            api->UiButton(api->Context, id, rect, text);
        }
    }

    public static bool UiClicked(FabrApi* api, uint id) => api->UiClicked(api->Context, id) != 0;

    public static bool LoadScene(FabrApi* api, string path)
    {
        byte[] bytes = Utf8(path);
        fixed (byte* text = bytes)
        {
            return api->LoadScene(api->Context, text) != 0;
        }
    }

    public static bool ReloadScene(FabrApi* api) => api->ReloadScene(api->Context) != 0;

    public static ulong? LoadSceneAdditive(FabrApi* api, string path)
    {
        byte[] bytes = Utf8(path);
        ulong id;
        fixed (byte* text = bytes)
        {
            if (api->LoadSceneAdditive(api->Context, text, &id) == 0)
            {
                return null;
            }
        }
        return id;
    }

    public static bool UnloadScene(FabrApi* api, ulong id) =>
        api->UnloadScene(api->Context, id) != 0;

    public static bool CreateJoint(
        FabrApi* api,
        ulong a,
        ulong b,
        float rest,
        float stiffness,
        float damping) =>
        api->CreateJoint(api->Context, a, b, rest, stiffness, damping) != 0;
}

public abstract unsafe class FabrScript
{
    public ulong Self { get; internal set; }

    internal FabrApi* Api { get; set; }

    public float Delta => FabrOps.Delta(Api);

    public float Elapsed => FabrOps.Elapsed(Api);

    public bool Key(string name) => FabrOps.Key(Api, name);

    public bool KeyDown(string name) => FabrOps.KeyDown(Api, name);

    public bool Mouse(MouseButton button) => FabrOps.Mouse(Api, button);

    public bool Action(string name) => FabrOps.Action(Api, name);

    public bool ActionDown(string name) => FabrOps.ActionDown(Api, name);

    public ulong? Find(string name) => FabrOps.Find(Api, name);

    public string? Name(ulong entity) => FabrOps.Name(Api, entity);

    public Vector3 Position => FabrOps.GetTranslation(Api, Self);

    public bool TryGetPosition(out Vector3 position) =>
        FabrOps.TryGetTranslation(Api, Self, out position);

    public Vector3 GetPosition(ulong entity) => FabrOps.GetTranslation(Api, entity);

    public bool TryGetPosition(ulong entity, out Vector3 position) =>
        FabrOps.TryGetTranslation(Api, entity, out position);

    public void SetPosition(Vector3 position) => FabrOps.SetTranslation(Api, Self, position);

    public bool SetPosition(ulong entity, Vector3 position) =>
        FabrOps.SetTranslation(Api, entity, position);

    public void Translate(Vector3 offset) => FabrOps.Translate(Api, Self, offset);

    public bool Translate(ulong entity, Vector3 offset) => FabrOps.Translate(Api, entity, offset);

    public Quaternion Rotation => FabrOps.GetRotation(Api, Self);

    public bool TryGetRotation(out Quaternion rotation) => FabrOps.TryGetRotation(Api, Self, out rotation);

    public Quaternion GetRotation(ulong entity) => FabrOps.GetRotation(Api, entity);

    public void SetRotation(Quaternion rotation) => FabrOps.SetRotation(Api, Self, rotation);

    public bool SetRotation(ulong entity, Quaternion rotation) =>
        FabrOps.SetRotation(Api, entity, rotation);

    public void RotateY(float radians) => FabrOps.RotateY(Api, Self, radians);

    public bool RotateY(ulong entity, float radians) => FabrOps.RotateY(Api, entity, radians);

    public Vector3 Velocity => FabrOps.GetVelocity(Api, Self);

    public bool TryGetVelocity(out Vector3 velocity) => FabrOps.TryGetVelocity(Api, Self, out velocity);

    public Vector3 GetVelocity(ulong entity) => FabrOps.GetVelocity(Api, entity);

    public void SetVelocity(Vector3 velocity) => FabrOps.SetVelocity(Api, Self, velocity);

    public bool SetVelocity(ulong entity, Vector3 velocity) => FabrOps.SetVelocity(Api, entity, velocity);

    public bool Grounded => FabrOps.Grounded(Api, Self);

    public bool IsGrounded(ulong entity) => FabrOps.Grounded(Api, entity);

    public Vector3 AngularVelocity => FabrOps.GetAngularVelocity(Api, Self);

    public bool TryGetAngularVelocity(out Vector3 angular) =>
        FabrOps.TryGetAngularVelocity(Api, Self, out angular);

    public Vector3 GetAngularVelocity(ulong entity) => FabrOps.GetAngularVelocity(Api, entity);

    public void SetAngularVelocity(Vector3 angular) => FabrOps.SetAngularVelocity(Api, Self, angular);

    public bool SetAngularVelocity(ulong entity, Vector3 angular) =>
        FabrOps.SetAngularVelocity(Api, entity, angular);

    public void AddSpin(Vector3 impulse) => FabrOps.AddSpin(Api, Self, impulse);

    public bool AddSpin(ulong entity, Vector3 impulse) => FabrOps.AddSpin(Api, entity, impulse);

    public bool Walk(float x, float z) => FabrOps.Walk(Api, Self, x, z);

    public bool Walk(ulong entity, float x, float z) => FabrOps.Walk(Api, entity, x, z);

    public bool Jump() => FabrOps.Jump(Api, Self);

    public bool Jump(ulong entity) => FabrOps.Jump(Api, entity);

    public string[] AnimationClips => FabrOps.AnimationClips(Api, Self);

    public string[] GetAnimationClips(ulong entity) => FabrOps.AnimationClips(Api, entity);

    public bool PlayAnimation(string name) => FabrOps.PlayAnimation(Api, Self, name);

    public bool PlayAnimation(ulong entity, string name) => FabrOps.PlayAnimation(Api, entity, name);

    public void StopAnimation() => FabrOps.StopAnimation(Api, Self);

    public bool StopAnimation(ulong entity) => FabrOps.StopAnimation(Api, entity);

    public Vector3[] FindPath(Vector3 from, Vector3 to) => FabrOps.FindPath(Api, from, to);

    public bool MoveTo(Vector3 goal, float speed = 3.0f) => FabrOps.MoveTo(Api, Self, goal, speed);

    public bool MoveTo(ulong entity, Vector3 goal, float speed = 3.0f) =>
        FabrOps.MoveTo(Api, entity, goal, speed);

    public void StopMoving() => FabrOps.StopMoving(Api, Self);

    public bool StopMoving(ulong entity) => FabrOps.StopMoving(Api, entity);

    public void PlaySound(string path, float volume = 1.0f, AudioBus bus = AudioBus.Sfx) =>
        FabrOps.PlaySound(Api, path, volume, bus);

    public void UiClear() => FabrOps.UiClear(Api);

    public void UiText(float x, float y, float size, string text, Vector3 color) =>
        FabrOps.UiText(Api, x, y, size, text, color);

    public void UiPanel(float x, float y, float w, float h, Vector4 color) =>
        FabrOps.UiPanel(Api, x, y, w, h, color);

    public void UiButton(uint id, float x, float y, float w, float h, string label) =>
        FabrOps.UiButton(Api, id, x, y, w, h, label);

    public bool UiClicked(uint id) => FabrOps.UiClicked(Api, id);

    public bool LoadScene(string path) => FabrOps.LoadScene(Api, path);

    public bool ReloadScene() => FabrOps.ReloadScene(Api);

    public ulong? LoadSceneAdditive(string path) => FabrOps.LoadSceneAdditive(Api, path);

    public bool UnloadScene(ulong id) => FabrOps.UnloadScene(Api, id);

    public bool CreateJoint(ulong a, ulong b, float rest, float stiffness, float damping) =>
        FabrOps.CreateJoint(Api, a, b, rest, stiffness, damping);

    public void Log(string message) => FabrOps.Log(Api, message);

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

using System.Numerics;
using Fabr.Bindings;

namespace Example;

public sealed class Spin : FabrScript
{
    private float reported;

    public override void OnStart()
    {
        Log("example spin started");
    }

    public override void OnUpdate(float dt)
    {
        Translate(new Vector3(dt * 2.0f, 0.0f, 0.0f));
        reported += dt;
        if (reported >= 1.0f)
        {
            reported = 0.0f;
            Vector3 position = Position;
            Log($"example spin x={position.X:F2}");
        }
    }

    public override void OnStop()
    {
        Log("example spin stopped");
    }
}

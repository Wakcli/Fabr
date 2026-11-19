using System.Numerics;
using Fabr.Bindings;

namespace Game;

public sealed class Spin : FabrScript
{
    [Property]
    public float Speed { get; set; } = 2.0f;

    private float _reported;

    public override void OnStart() => Log($"spin start at {Position}");

    public override void OnUpdate(float dt)
    {
        Translate(new Vector3(dt * Speed, 0.0f, 0.0f));
        RotateY(dt);
        _reported += dt;
        if (_reported < 1.0f)
        {
            return;
        }
        _reported = 0.0f;
        Log($"spin x={Position.X:F2} clips={AnimationClips.Length}");
    }

    public override void OnStop() => Log("spin stop");
}

using System.Numerics;
using Fabr.Bindings;

namespace Game;

public sealed class Walker : FabrScript
{
    private const float Speed = 4.0f;
    private const uint JumpButton = 1;

    private string[] _clips = Array.Empty<string>();
    private bool _walking;

    public override void OnStart()
    {
        _clips = AnimationClips;
        Log($"walker ready with {_clips.Length} clips");
    }

    public override void OnUpdate(float dt)
    {
        Vector2 axis = new(
            Axis("move_right", "move_left"),
            Axis("move_forward", "move_back"));
        if (axis.LengthSquared() > 1.0f)
        {
            axis = Vector2.Normalize(axis);
        }

        Walk(axis.X * Speed, axis.Y * Speed);
        if (Grounded && ActionDown("jump"))
        {
            Jump();
        }

        SetWalking(axis.LengthSquared() > 0.01f);
        DrawHud();
    }

    private float Axis(string positive, string negative) =>
        (Action(positive) ? 1.0f : 0.0f) - (Action(negative) ? 1.0f : 0.0f);

    private void SetWalking(bool walking)
    {
        if (walking == _walking)
        {
            return;
        }
        _walking = walking;
        string clip = walking ? "Walk" : "Idle";
        if (Array.IndexOf(_clips, clip) >= 0)
        {
            PlayAnimation(clip);
        }
    }

    private void DrawHud()
    {
        UiPanel(0.02f, 0.86f, 0.26f, 0.1f, new Vector4(0.0f, 0.0f, 0.0f, 0.55f));
        UiText(0.04f, 0.88f, 18.0f, $"{Name(Self) ?? "Walker"} {(Grounded ? "grounded" : "airborne")}", Vector3.One);
        UiButton(JumpButton, 0.04f, 0.9f, 0.1f, 0.04f, "Jump");
        if (UiClicked(JumpButton) && Grounded)
        {
            Jump();
        }
    }
}

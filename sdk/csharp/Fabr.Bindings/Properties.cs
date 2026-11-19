using System.Globalization;
using System.Reflection;
using System.Text;

namespace Fabr.Bindings;

/// Marks a public field or property as an editable script property.
[AttributeUsage(AttributeTargets.Field | AttributeTargets.Property, Inherited = true)]
public sealed class PropertyAttribute : Attribute
{
}

/// Reads and writes `[Property]` members for the engine inspector.
internal static class ScriptProperties
{
    private const BindingFlags Flags = BindingFlags.Public | BindingFlags.Instance;

    private static readonly CultureInfo Invariant = CultureInfo.InvariantCulture;

    internal static string Encode(object script)
    {
        StringBuilder text = new();
        foreach (MemberInfo member in Members(script.GetType()))
        {
            object? value = Read(script, member);
            text.Append(member.Name)
                .Append('\t')
                .Append(Kind(TypeOf(member)))
                .Append('\t')
                .Append(Format(value))
                .Append('\n');
        }
        return text.ToString();
    }

    internal static void Apply(object script, string payload)
    {
        Dictionary<string, MemberInfo> members = Members(script.GetType())
            .ToDictionary(member => member.Name, StringComparer.Ordinal);
        List<string> errors = [];
        foreach (string line in payload.Split('\n'))
        {
            if (line.Length == 0)
            {
                continue;
            }
            int tab = line.IndexOf('\t');
            if (tab <= 0)
            {
                errors.Add($"property override has no tab: {line}");
                continue;
            }
            string name = line[..tab];
            if (!members.TryGetValue(name, out MemberInfo? member))
            {
                continue;
            }
            try
            {
                Write(script, member, line[(tab + 1)..]);
            }
            catch (Exception error)
            {
                errors.Add($"{name}: {error.Message}");
            }
        }
        if (errors.Count > 0)
        {
            throw new InvalidOperationException(string.Join("; ", errors));
        }
    }

    private static List<MemberInfo> Members(Type type)
    {
        List<MemberInfo> members = [];
        foreach (PropertyInfo property in type.GetProperties(Flags))
        {
            if (property.GetCustomAttribute<PropertyAttribute>(inherit: true) is null)
            {
                continue;
            }
            if (property.GetMethod is null || property.SetMethod?.IsPublic != true)
            {
                continue;
            }
            if (!Supported(property.PropertyType))
            {
                continue;
            }
            members.Add(property);
        }
        foreach (FieldInfo field in type.GetFields(Flags))
        {
            if (field.GetCustomAttribute<PropertyAttribute>(inherit: true) is null)
            {
                continue;
            }
            if (!Supported(field.FieldType))
            {
                continue;
            }
            members.Add(field);
        }
        members.Sort((left, right) => string.CompareOrdinal(left.Name, right.Name));
        return members;
    }

    private static bool Supported(Type type) =>
        type == typeof(float)
        || type == typeof(double)
        || type == typeof(int)
        || type == typeof(bool)
        || type == typeof(string);

    private static Type TypeOf(MemberInfo member) => member switch
    {
        PropertyInfo property => property.PropertyType,
        FieldInfo field => field.FieldType,
        _ => throw new ArgumentException($"{member.Name} is not a field or property"),
    };

    private static string Kind(Type type) =>
        type == typeof(bool) ? "bool" : type == typeof(string) ? "text" : "number";

    private static object? Read(object script, MemberInfo member) => member switch
    {
        PropertyInfo property => property.GetValue(script),
        FieldInfo field => field.GetValue(script),
        _ => null,
    };

    private static void Write(object script, MemberInfo member, string raw)
    {
        Type type = TypeOf(member);
        object value;
        if (type == typeof(bool))
        {
            value = bool.Parse(raw);
        }
        else if (type == typeof(string))
        {
            value = raw;
        }
        else if (type == typeof(int))
        {
            value = int.Parse(raw, NumberStyles.Integer, Invariant);
        }
        else if (type == typeof(float))
        {
            value = float.Parse(raw, NumberStyles.Float, Invariant);
        }
        else
        {
            value = double.Parse(raw, NumberStyles.Float, Invariant);
        }
        switch (member)
        {
            case PropertyInfo property:
                property.SetValue(script, value);
                break;
            case FieldInfo field:
                field.SetValue(script, value);
                break;
        }
    }

    private static string Format(object? value) => value switch
    {
        null => string.Empty,
        bool flag => flag ? "true" : "false",
        float number => number.ToString("R", Invariant),
        double number => number.ToString("R", Invariant),
        int number => number.ToString(Invariant),
        string text => Sanitize(text),
        _ => string.Empty,
    };

    private static string Sanitize(string text) =>
        text.Replace('\n', ' ').Replace('\r', ' ');
}

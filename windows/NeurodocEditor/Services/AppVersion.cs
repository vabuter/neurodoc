using System.Reflection;

namespace NeurodocEditor.Services;

/// <summary>
/// Версия приложения. Строку кладёт в сборку таргет SetVersionFromGit из csproj,
/// беря её из git-тега vX.Y.Z; здесь она только читается.
/// </summary>
/// <remarks>
/// Приложение unpackaged, поэтому Package.Current недоступен и версию приходится брать
/// из атрибута сборки, а не из манифеста пакета.
/// </remarks>
public static class AppVersion
{
    /// <summary>
    /// Что показать пользователю: «0.1.0» у сборки с тега, «0.1.0-3-g1a2b3c4» —
    /// у промежуточной, с суффиксом «-dirty» — если в дереве были несохранённые правки.
    /// </summary>
    public static string Display { get; } =
        typeof(AppVersion).Assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()
            ?.InformationalVersion ?? "0.0.0";
}

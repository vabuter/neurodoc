using System.Text;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

namespace NeurodocEditor.Services;

/// <summary>
/// Чтение и запись YAML-файлов приложения — настроек и прогресса чтения.
/// </summary>
/// <remarks>
/// Сериализаторы создаются один раз на всё приложение: состояния между вызовами они не
/// держат, а прогресс чтения переписывается при каждом переходе по ссылке — собирать их
/// заново на каждую запись незачем.
/// </remarks>
public static class YamlStore
{
    private static readonly UTF8Encoding Utf8NoBom = new(false);

    private static readonly ISerializer Serializer = new SerializerBuilder()
        .WithNamingConvention(CamelCaseNamingConvention.Instance)
        .Build();

    /// <remarks>
    /// Незнакомые поля пропускаем: файл мог остаться от прежней версии приложения,
    /// и настройка, которой больше нет, не повод потерять все остальные.
    /// </remarks>
    private static readonly IDeserializer Deserializer = new DeserializerBuilder()
        .WithNamingConvention(CamelCaseNamingConvention.Instance)
        .IgnoreUnmatchedProperties()
        .Build();

    /// <summary>
    /// Читает файл. null — файла нет либо он испорчен: ни то, ни другое не повод
    /// сорвать запуск, вызывающий подставляет значения по умолчанию.
    /// </summary>
    public static T? Load<T>(string path) where T : class
    {
        try
        {
            return File.Exists(path) ? Deserializer.Deserialize<T>(File.ReadAllText(path)) : null;
        }
        catch
        {
            return null;
        }
    }

    /// <summary>
    /// Пишет файл, создавая папку. Сбой записи проглатывается: настройки и прогресс —
    /// не та потеря, ради которой стоит прерывать работу с документацией.
    /// </summary>
    public static void Save<T>(string path, T value)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, Serializer.Serialize(value), Utf8NoBom);
        }
        catch
        {
            // см. remarks
        }
    }
}

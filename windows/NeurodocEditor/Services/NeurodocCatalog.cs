using System.Text;
using System.Text.RegularExpressions;

namespace NeurodocEditor.Services;

public record SearchResult(string FileId, string Title, string Prefix, string Match, string Suffix, int Occurrence);

public record Backlink(string FileId, string Title);

/// <summary>Ссылка на несуществующий файл: где найдена и куда ведёт.</summary>
public record BrokenLink(string FileId, string FileTitle, string TargetId, int Position, string Context);

/// <summary>
/// Каталог документации в формате neurodoc: корневой файл root + файлы-UUID в папке _content.
/// Держит содержимое всех файлов в памяти (mapping "имя файла" -> "контент").
/// </summary>
public class NeurodocCatalog
{
    public const string RootId = "root";

    /// <summary>
    /// Ссылка формата neurodoc: [текст](цель). Группа 1 — текст ссылки (якорь, может быть
    /// пустым), группа 2 — цель: UUID файла из _content/ либо путь к файлу каталога
    /// ОТНОСИТЕЛЬНО того файла, где написана ссылка. Якорь однострочный и не содержит "]".
    /// Цель разбирается в id через <see cref="TryResolveLink"/> — сама по себе она неполна.
    /// </summary>
    public static readonly Regex LinkRegex = new(
        @"\[([^\]\n]*)\]\(([^()\s\\]+)\)",
        RegexOptions.Compiled);

    /// <summary>
    /// Ссылка старого формата &lt;uuid&gt;. Ссылкой больше не считается и нужна только
    /// миграции каталога на новый формат (см. CatalogMigration).
    /// </summary>
    public static readonly Regex LegacyLinkRegex = new(
        @"<([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})>",
        RegexOptions.Compiled);

    private static readonly Regex IdRegex = new(
        @"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        RegexOptions.Compiled);

    /// <summary>UUID в произвольном месте строки (для разбора пользовательского ввода).</summary>
    private static readonly Regex AnyIdRegex = new(
        @"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        RegexOptions.Compiled);

    private static readonly UTF8Encoding Utf8NoBom = new(false);
    private static readonly char[] NewlineChars = { '\r', '\n' };

    /// <summary>
    /// Папка с neurodoc-файлами. Подчёркивание в начале помечает папку служебной, но,
    /// в отличие от точки, не делает её скрытой: каталог смотрят и обычным файловым
    /// менеджером, и с телефона. Обычное «content» остаётся свободным под прикладную
    /// папку самого проекта.
    /// </summary>
    public const string ContentDir = "_content";

    public string RootPath { get; }
    public string ContentPath => Path.Combine(RootPath, ContentDir);

    private readonly Dictionary<string, string> _files = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Разобранные ссылки файлов. Панели (пути, обратные ссылки, сироты, битые, порядок
    /// чтения) перестраиваются после каждого автосохранения, и без кэша каждая заново
    /// прогоняла регулярку по всему каталогу — пять полных проходов на одну правку.
    /// </summary>
    /// <remarks>
    /// Разбор ссылок файла зависит только от его текста и его же папки, поэтому запись
    /// сбрасывает лишь запись изменённого файла, а не весь кэш.
    /// </remarks>
    private readonly Dictionary<string, List<LinkRef>> _links = new(StringComparer.OrdinalIgnoreCase);

    private NeurodocCatalog(string rootPath) => RootPath = rootPath;

    public static NeurodocCatalog OpenOrCreate(string path)
    {
        EnsureStructure(path);
        var catalog = new NeurodocCatalog(path);
        catalog.Reload();
        return catalog;
    }

    /// <summary>Создаёт каталог проекта, файл root, папку _content и ReadMe.md, если их ещё нет.</summary>
    public static void EnsureStructure(string path)
    {
        Directory.CreateDirectory(path);
        Directory.CreateDirectory(Path.Combine(path, ContentDir));

        var rootFile = Path.Combine(path, "root");
        if (!File.Exists(rootFile))
            File.WriteAllText(rootFile, "", Utf8NoBom);

        bool readmeExists = Directory.EnumerateFiles(path).Any(f =>
            string.Equals(Path.GetFileName(f), "readme.md", StringComparison.OrdinalIgnoreCase));
        if (!readmeExists)
            File.WriteAllText(Path.Combine(path, "ReadMe.md"), ReadMeTemplate, Utf8NoBom);
    }

    /// <summary>Больше этого файл в память не берём — каталог не хранилище двоичных данных.</summary>
    private const long MaxFileBytes = 4 * 1024 * 1024;

    public void Reload()
    {
        _files.Clear();
        _links.Clear();
        _files[RootId] = ReadFromDisk(RootId);
        if (Directory.Exists(ContentPath))
        {
            foreach (var file in Directory.EnumerateFiles(ContentPath))
            {
                var name = Path.GetFileName(file);
                if (IdRegex.IsMatch(name))
                {
                    try { _files[name.ToLowerInvariant()] = File.ReadAllText(file); }
                    catch { /* нечитабельный файл пропускаем */ }
                }
            }
        }

        // Кроме _content в каталоге бывают папки с файлами в собственных форматах
        // (YAML, SVG, конфиги) — на них тоже ссылаются, по пути от файла-источника.
        foreach (var file in EnumerateDomainFiles(RootPath))
        {
            var rel = Path.GetRelativePath(RootPath, file).Replace('\\', '/');
            if (string.Equals(rel, "root", StringComparison.OrdinalIgnoreCase)) continue;
            try
            {
                if (new FileInfo(file).Length > MaxFileBytes) continue;
                var text = File.ReadAllText(file);
                if (text.Contains('\0')) continue; // двоичный файл читать нечем
                _files[rel] = text;
            }
            catch { /* нечитабельный файл пропускаем */ }
        }
    }

    /// <summary>
    /// Файлы каталога в собственных форматах: всё дерево, кроме служебной папки
    /// <see cref="ContentDir"/>, папок с точки (.git и подобные) и файлов, имя которых
    /// начинается с точки.
    /// </summary>
    /// <remarks>
    /// Обходим папки сами, а не через SearchOption.AllDirectories: тот сначала спускается
    /// в .git и перечисляет всё, что там лежит, и только потом имена отбрасываются по
    /// маске. На настоящем репозитории это десятки тысяч лишних имён при каждом
    /// перечитывании — а перечитывание случается и по изменению файла на диске.
    /// </remarks>
    private static IEnumerable<string> EnumerateDomainFiles(string root)
    {
        var pending = new Queue<string>();
        pending.Enqueue(root);
        while (pending.Count > 0)
        {
            var dir = pending.Dequeue();
            List<string> subdirs, files;
            try
            {
                subdirs = Directory.EnumerateDirectories(dir).ToList();
                files = Directory.EnumerateFiles(dir).ToList();
            }
            catch
            {
                continue; // нечитаемую папку пропускаем целиком
            }

            foreach (var sub in subdirs)
            {
                var name = Path.GetFileName(sub);
                // _content отбрасываем ПО ИМЕНИ: точка его больше не прячет, и без
                // этой проверки файлы-UUID попали бы в каталог второй раз — как файлы
                // в собственном формате с путём «_content/<uuid>»
                if (name.StartsWith('.')) continue;
                if (string.Equals(name, ContentDir, StringComparison.OrdinalIgnoreCase)) continue;
                pending.Enqueue(sub);
            }

            foreach (var file in files)
                if (!Path.GetFileName(file).StartsWith('.'))
                    yield return file;
        }
    }

    public IEnumerable<string> AllIds() =>
        _files.Keys.OrderBy(k => k == RootId ? "" : k, StringComparer.Ordinal);

    /// <summary>UUID ли это — то есть файл из папки _content.</summary>
    public static bool IsUuid(string id) => IdRegex.IsMatch(id);

    /// <summary>
    /// Файл формата neurodoc (root или файл из _content) — в отличие от файлов в собственных
    /// форматах, которые лежат в своих папках и читаются в том числе программами.
    /// </summary>
    public static bool IsNeurodocFile(string id) =>
        string.Equals(id, RootId, StringComparison.OrdinalIgnoreCase) || IsUuid(id);

    /// <summary>Папка файла относительно корня каталога: "" для root, "_content" для UUID.</summary>
    public static string DirOf(string id)
    {
        if (string.Equals(id, RootId, StringComparison.OrdinalIgnoreCase)) return "";
        if (IsUuid(id)) return ContentDir;
        int slash = id.LastIndexOf('/');
        return slash < 0 ? "" : id[..slash];
    }

    /// <summary>Ссылка на внешний ресурс, а не на файл каталога.</summary>
    public static bool IsWebTarget(string target) =>
        target.StartsWith("http://", StringComparison.OrdinalIgnoreCase) ||
        target.StartsWith("https://", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Разбирает цель ссылки в id файла. Путь считается ОТ ФАЙЛА, в котором написана ссылка,
    /// поэтому нужен <paramref name="sourceId"/>.
    /// </summary>
    /// <returns>
    /// false, если цель — веб-адрес, абсолютный путь или ведёт за пределы каталога:
    /// по формату ссылок наружу не бывает, и разыменовывать их нельзя.
    /// </returns>
    public static bool TryResolveLink(string sourceId, string target, out string id)
    {
        id = "";
        if (string.IsNullOrWhiteSpace(target)) return false;
        target = target.Trim();
        if (IsWebTarget(target)) return false;
        if (IsUuid(target))
        {
            id = target.ToLowerInvariant();
            return true;
        }
        // абсолютные пути и диски каталогу не принадлежат
        if (target.Contains(':') || target[0] == '/' || target[0] == '\\') return false;

        var parts = new List<string>();
        foreach (var segment in DirOf(sourceId).Split('/', StringSplitOptions.RemoveEmptyEntries))
            parts.Add(segment);
        foreach (var segment in target.Replace('\\', '/').Split('/'))
        {
            if (segment.Length == 0 || segment == ".") continue;
            if (segment == "..")
            {
                // подъём выше корня каталога — ссылка наружу, такой в формате нет
                if (parts.Count == 0) return false;
                parts.RemoveAt(parts.Count - 1);
                continue;
            }
            parts.Add(segment);
        }
        if (parts.Count == 0) return false;

        // приводим к каноническому виду: _content/<uuid> — это тот же id, что и <uuid>
        if (parts.Count == 2 && string.Equals(parts[0], ContentDir, StringComparison.OrdinalIgnoreCase)
            && IsUuid(parts[1]))
        {
            id = parts[1].ToLowerInvariant();
            return true;
        }
        if (parts.Count == 1 && string.Equals(parts[0], RootId, StringComparison.OrdinalIgnoreCase))
        {
            id = RootId;
            return true;
        }
        id = string.Join('/', parts);
        return true;
    }

    /// <summary>
    /// Путь цели относительно папки, из которой на неё сошлются, — обратная операция
    /// к разбору ссылки. Папку берут через <see cref="DirOf"/>.
    /// </summary>
    public static string MakeRelativeTarget(string sourceDir, string targetId)
    {
        if (IsUuid(targetId)) return targetId;

        var from = sourceDir.Split('/', StringSplitOptions.RemoveEmptyEntries);
        var to = targetId.Split('/', StringSplitOptions.RemoveEmptyEntries);
        int common = 0;
        while (common < from.Length && common < to.Length &&
               string.Equals(from[common], to[common], StringComparison.OrdinalIgnoreCase))
            common++;

        var parts = new List<string>();
        for (int i = common; i < from.Length; i++) parts.Add("..");
        for (int i = common; i < to.Length; i++) parts.Add(to[i]);
        return parts.Count == 0 ? targetId : string.Join('/', parts);
    }

    /// <summary>
    /// Годен ли id как имя файла каталога: за его пределы формат ссылок не выпускает.
    /// </summary>
    public static bool IsValidId(string id) =>
        !string.IsNullOrWhiteSpace(id)
        && !id.Contains("..")
        && !id.Contains(':')
        && id[0] != '/' && id[0] != '\\';

    /// <exception cref="ArgumentException">
    /// id ведёт за пределы каталога. Такие сюда попадать не должны: их отсеивает
    /// <see cref="TryResolveLink"/>. Раньше здесь была молчаливая подмена на root —
    /// от неё <see cref="Exists"/> отвечал «файл есть», а запись пошла бы в root.
    /// </exception>
    public string GetFilePath(string id)
    {
        if (string.Equals(id, RootId, StringComparison.OrdinalIgnoreCase))
            return Path.Combine(RootPath, "root");
        if (IsUuid(id)) return Path.Combine(ContentPath, id);
        if (!IsValidId(id))
            throw new ArgumentException($"Путь ведёт за пределы каталога: {id}", nameof(id));
        return Path.Combine(RootPath, id.Replace('/', Path.DirectorySeparatorChar));
    }

    public string ReadFile(string id) =>
        _files.TryGetValue(id, out var text) ? text : ReadFromDisk(id);

    private string ReadFromDisk(string id)
    {
        var path = GetFilePath(id);
        try { return File.Exists(path) ? File.ReadAllText(path) : ""; }
        catch { return ""; }
    }

    /// <summary>
    /// Не изменит ли перечитывание ничего для этого пути: файл на диске совпадает с тем,
    /// что каталог держит в памяти, либо его нет ни там, ни там. Нужно слежению за
    /// диском — собственная запись поднимает то же событие, что и чужая.
    /// </summary>
    /// <remarks>
    /// Сверяется содержимое, а не время записи. Раньше событие считалось своим, если
    /// приложение писало что угодно меньше секунды назад, — и чужая правка, пришедшая
    /// сразу после автосохранения (git pull в терминале, другая программа), терялась:
    /// перечитывание не назначалось вовсе. Сверка по содержимому узнаёт свою запись и
    /// при запоздавшем событии, а чужую не пропустит даже в тот же файл.
    /// <para>
    /// Сомнение решается в пользу «изменился»: лишнее перечитывание безвредно, а
    /// пропущенное оставляет в памяти устаревший текст, который автосохранение потом
    /// запишет поверх чужой правки. Поэтому папка, незнакомый файл и путь вне каталога —
    /// всегда «изменился».
    /// </para>
    /// </remarks>
    public bool MatchesDisk(string path)
    {
        string rel;
        try { rel = Path.GetRelativePath(RootPath, path).Replace('\\', '/'); }
        catch { return false; }
        // событие о самой папке каталога или о пути вне её
        if (rel == "." || rel == ".." || rel.StartsWith("../", StringComparison.Ordinal) || Path.IsPathRooted(rel))
            return false;
        var id = IdForRelativePath(rel);
        try
        {
            // Файл, который каталог держит в памяти, сверяется всегда — даже с точкой в пути
            // или больше 4 МБ: такие перечитывание не заводит, но в память их кладёт запись
            // (правка файла, открытого по ссылке вроде .gitlab-ci.yml).
            if (id != null && _files.TryGetValue(id, out var cached))
            {
                // ФС Windows регистр не различает: переименование Start.yaml → start.yaml видно
                // только по написанию в событии, а перечитывание заведёт файл под новым.
                // Поэтому своим считаем только то же написание, что в памяти.
                bool sameSpelling = _files.Keys.Any(k => string.Equals(k, id, StringComparison.Ordinal));
                return sameSpelling && File.Exists(path) && File.ReadAllText(path) == cached;
            }
            // Дальше — незнакомые. Пропавший путь что-то меняет, только если это папка с
            // известными каталогу файлами (её унесли целиком). Короткое имя 8.3 (SPECIF~1.YAM),
            // которым Windows может назвать удалённое, с каталогом не сверить — изменение.
            if (!File.Exists(path) && !Directory.Exists(path))
            {
                var prefix = rel + "/";
                // по самому пути: перечитывание заводит и файлы, чей путь разбор ссылки
                // не примет (12:30.md), — их ключ не восстановить через IdForRelativePath
                return !rel.Contains('~')
                    && !_files.ContainsKey(rel)
                    && !string.Equals(rel, ContentDir, StringComparison.OrdinalIgnoreCase)
                    && !_files.Keys.Any(k => k.StartsWith(prefix, StringComparison.OrdinalIgnoreCase));
            }
            // в папки с точки перечитывание не заходит и такие файлы не читает (.git, свапы
            // редакторов): для каталога их изменения ничего не значат
            if (rel.Split('/').Any(part => part.StartsWith('.'))) return true;
            // незнакомый файл перечитывание не заведёт, только если отбросит: слишком большой
            // или двоичный (root и файлы _content так не отсеиваются); папка — всегда изменение
            if (id == null || !File.Exists(path) || IsNeurodocFile(id)) return false;
            return new FileInfo(path).Length > MaxFileBytes || StartsBinary(path);
        }
        catch
        {
            return false; // файл занят или недоступен — пусть разбирается перечитывание
        }
    }

    /// <summary>Сколько символов начала файла смотрит <see cref="StartsBinary"/>.</summary>
    private const int BinaryProbeChars = 8 * 1024;

    /// <summary>
    /// Двоичный ли файл — по символу 0 в начале. Перечитывание отсеивает файл с нулём где
    /// угодно, но читать его целиком на каждое событие нельзя: генераторы картинок пишут
    /// кусками, событий сотни, и каждое читало бы до 4 МБ в потоке интерфейса (замер на
    /// Linux-версии: 9 с на PNG в 3 МБ). Нуль в начале — точно двоичный, перечитывание такой
    /// файл тоже отбросит. Без нуля ответ — «изменился»: дальше разберётся одно перечитывание.
    /// </summary>
    /// <remarks>
    /// Декодер тот же, что у <see cref="File.ReadAllText(string)"/> (UTF-8 с распознаванием
    /// BOM): в тексте UTF-16 есть нулевые байты, но не нулевые символы, и перечитывание
    /// заводит его как текст.
    /// </remarks>
    private static bool StartsBinary(string path)
    {
        using var reader = new StreamReader(path, Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
        var head = new char[BinaryProbeChars];
        int read = reader.ReadBlock(head, 0, head.Length);
        return Array.IndexOf(head, '\0', 0, read) >= 0;
    }

    /// <summary>
    /// Идентификатор, под которым перечитывание завело бы файл с этим путём (относительно
    /// корня каталога, через «/»), или null, если такого файла в каталоге быть не может.
    /// </summary>
    private static string? IdForRelativePath(string rel)
    {
        if (string.Equals(rel, RootId, StringComparison.OrdinalIgnoreCase)) return RootId;
        if (rel.StartsWith(ContentDir + "/", StringComparison.OrdinalIgnoreCase))
        {
            var name = rel[(ContentDir.Length + 1)..];
            return IsUuid(name) ? name.ToLowerInvariant() : null;
        }
        return IsValidId(rel) ? rel : null;
    }

    public void WriteFile(string id, string text)
    {
        var path = GetFilePath(id);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, text, Utf8NoBom);
        _files[id] = text;
        _links.Remove(id);
    }

    /// <summary>
    /// Разбирает пользовательский ввод в id файла: "root", голый UUID, ссылка "&lt;uuid&gt;"
    /// или произвольный текст, внутри которого есть UUID.
    /// </summary>
    public static bool TryParseId(string input, out string id)
    {
        id = "";
        if (string.IsNullOrWhiteSpace(input)) return false;

        var trimmed = input.Trim().Trim('<', '>', '(', ')').Trim();
        if (string.Equals(trimmed, RootId, StringComparison.OrdinalIgnoreCase))
        {
            id = RootId;
            return true;
        }

        var match = AnyIdRegex.Match(input);
        if (!match.Success) return false;
        id = match.Value.ToLowerInvariant();
        return true;
    }

    /// <summary>
    /// Разбирает то, что ввёл пользователь, в id файла ЭТОГО каталога: «root», UUID
    /// (в том числе внутри произвольного текста) или путь вида contract/auth/start.yaml.
    /// </summary>
    /// <remarks>
    /// <see cref="TryParseId"/> про пути ничего не знает — он ищет UUID в отрыве от
    /// каталога. А в списке файлов проекта такие пути есть, и без этой проверки
    /// выбранный там файл не открывался: диалог отвечал «это не похоже на UUID файла».
    /// </remarks>
    public bool TryResolveInput(string input, out string id)
    {
        id = "";
        if (string.IsNullOrWhiteSpace(input)) return false;

        if (TryParseId(input, out var parsed) && Exists(parsed))
        {
            id = parsed;
            return true;
        }

        var path = input.Trim().Replace('\\', '/');
        if (!IsValidId(path) || !Exists(path)) return false;
        id = path;
        return true;
    }

    /// <summary>Есть ли такой файл в каталоге. Негодный id — всегда «нет».</summary>
    public bool Exists(string id) =>
        IsValidId(id) && (_files.ContainsKey(id) || File.Exists(GetFilePath(id)));

    public string CreateNewFile()
    {
        var id = Guid.NewGuid().ToString("D").ToLowerInvariant();
        WriteFile(id, "");
        return id;
    }

    public void DeleteFile(string id)
    {
        if (id == RootId) return;
        var path = GetFilePath(id);
        if (File.Exists(path)) File.Delete(path);
        _files.Remove(id);
        _links.Remove(id);
    }

    /// <summary>
    /// Снимает во всех файлах каталога ссылки на указанный: [текст](цель) превращается
    /// в текст. Возвращает число изменённых файлов.
    /// </summary>
    /// <remarks>
    /// Текст ссылки остаётся намеренно — его писал автор, и он часть предложения.
    /// Удаление вместе со ссылкой рвало бы фразу в чужом файле. Ссылка без текста
    /// (пустой якорь) исчезает целиком: сохранять там нечего.
    /// </remarks>
    public int RemoveLinksTo(string id)
    {
        int changedFiles = 0;
        foreach (var fileId in LinkSourceIds().ToList())
        {
            if (string.Equals(fileId, id, StringComparison.OrdinalIgnoreCase)) continue;
            var text = _files[fileId];
            // один и тот же файл адресуется разными путями, поэтому сверяем разрешённую цель
            var updated = LinkRegex.Replace(text, m =>
                TryResolveLink(fileId, m.Groups[2].Value, out var target)
                && string.Equals(target, id, StringComparison.OrdinalIgnoreCase)
                    ? m.Groups[1].Value
                    : m.Value);
            if (updated != text)
            {
                WriteFile(fileId, updated);
                changedFiles++;
            }
        }
        return changedFiles;
    }

    /// <summary>
    /// Заголовок файла — его первая непустая строка. У файла в собственном формате
    /// первая строка служебная (комментарий, ключ), поэтому там заголовок — имя файла.
    /// </summary>
    public string GetTitle(string id, int maxLength = 60)
    {
        if (!IsNeurodocFile(id)) return id[(id.LastIndexOf('/') + 1)..];

        // построчно, без Split: заголовок нужен каждой строке боковых панелей,
        // а разрезание всего файла ради первой строки — мусор на ровном месте
        var text = ReadFile(id);
        int from = 0;
        while (from < text.Length)
        {
            int end = text.IndexOf('\n', from);
            if (end < 0) end = text.Length;
            var line = text[from..end].Trim().TrimStart('#').Trim();
            from = end + 1;
            if (line.Length == 0) continue;
            if (line.Length > maxLength) line = line[..maxLength].TrimEnd() + "…";
            return line;
        }
        return id == RootId ? "root" : Shorten(id);
    }

    public static string Shorten(string id) => id.Length > 8 ? id[..8] + "…" : id;

    /// <summary>Ссылка в тексте файла с уже разобранной целью.</summary>
    /// <param name="Target">Цель как написана в тексте — её показывают в «Битых ссылках».</param>
    /// <param name="Id">
    /// Файл каталога, на который она ведёт, или null: веб-адрес либо путь наружу каталога.
    /// </param>
    public readonly record struct LinkRef(int Start, int Length, string Target, string Anchor, string? Id);

    /// <summary>
    /// Файлы, в которых имеет смысл искать ссылки: root и _content.
    /// </summary>
    /// <remarks>
    /// Файлы в собственных форматах (YAML, SVG, конфиги) — цели ссылок, а не источники.
    /// Разбирать их как neurodoc опасно: последовательность вида [текст](что-то) там
    /// встречается сама по себе. На этом попался ReadMe.md — примеры из описания формата
    /// приложение считало настоящими ссылками и жаловалось на битые.
    /// </remarks>
    private IEnumerable<string> LinkSourceIds() => _files.Keys.Where(IsNeurodocFile);

    /// <summary>
    /// Ссылки файла с уже разрешёнными целями. Цель зависит от того, где написана ссылка,
    /// поэтому разбирать её в отрыве от файла-источника нельзя.
    /// </summary>
    public List<(int Start, int Length, string Id, string Anchor)> GetLinks(string sourceId) =>
        ParsedLinks(sourceId)
            .Where(l => l.Id != null)
            .Select(l => (l.Start, l.Length, l.Id!, l.Anchor))
            .ToList();

    /// <summary>
    /// Все ссылки файла, включая неразрешимые. Список кэшируется и наружу не отдаётся:
    /// он общий на всех вызывающих, и менять его нельзя.
    /// </summary>
    private List<LinkRef> ParsedLinks(string sourceId)
    {
        if (_links.TryGetValue(sourceId, out var cached)) return cached;

        var result = new List<LinkRef>();
        foreach (Match m in LinkRegex.Matches(ReadFile(sourceId)))
        {
            // регистр цели не трогаем: у пути он часть имени, а UUID приводит TryResolveLink
            var target = m.Groups[2].Value;
            result.Add(new LinkRef(m.Index, m.Length, target, m.Groups[1].Value,
                TryResolveLink(sourceId, target, out var id) ? id : null));
        }
        _links[sourceId] = result;
        return result;
    }

    /// <summary>Граф ссылок: id файла -> список id, на которые он ссылается.</summary>
    public Dictionary<string, List<string>> BuildGraph()
    {
        var graph = new Dictionary<string, List<string>>(StringComparer.OrdinalIgnoreCase);
        foreach (var id in LinkSourceIds())
            graph[id] = ParsedLinks(id)
                .Where(l => l.Id != null)
                .Select(l => l.Id!)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToList();
        return graph;
    }

    /// <summary>Файлы, ссылающиеся на указанный.</summary>
    public List<Backlink> GetBacklinks(string id)
    {
        // Подстрокой больше не найти: один и тот же файл из разных папок адресуется
        // разными путями, поэтому ссылки каждого файла разрешаем по-настоящему.
        return LinkSourceIds()
            .Where(key => !string.Equals(key, id, StringComparison.OrdinalIgnoreCase)
                          && ParsedLinks(key).Any(l => string.Equals(l.Id, id, StringComparison.OrdinalIgnoreCase)))
            .Select(key => new Backlink(key, GetTitle(key)))
            .OrderBy(b => b.Title, StringComparer.CurrentCultureIgnoreCase)
            .ToList();
    }

    /// <summary>
    /// Ссылки на несуществующие файлы. Дубли допустимы: одна и та же цель
    /// в разных местах — это разные записи, к каждой можно перейти.
    /// </summary>
    public List<BrokenLink> GetBrokenLinks()
    {
        var result = new List<BrokenLink>();
        foreach (var id in AllIds().Where(IsNeurodocFile))
        {
            string? text = null, title = null;
            foreach (var link in ParsedLinks(id))
            {
                if (IsWebTarget(link.Target)) continue; // веб-адрес файлом каталога не бывает
                // не разобралась — значит ведёт наружу каталога, это тоже битая ссылка
                var target = link.Id ?? link.Target;
                if (Exists(target)) continue;

                // текст и заголовок нужны только файлам с битыми ссылками, а их единицы
                text ??= ReadFile(id);
                title ??= GetTitle(id, 40);

                int lineStart = link.Start == 0 ? 0 : text.LastIndexOfAny(NewlineChars, link.Start - 1) + 1;
                int lineEnd = text.IndexOfAny(NewlineChars, link.Start);
                if (lineEnd < 0) lineEnd = text.Length;
                var context = text[lineStart..lineEnd].Trim();
                if (context.Length > 70) context = context[..70].TrimEnd() + "…";

                result.Add(new BrokenLink(id, title, target, link.Start, context));
            }
        }
        return result;
    }

    /// <summary>Файлы (кроме root), на которые нет ни одной ссылки из других файлов.</summary>
    public List<Backlink> GetOrphanFiles()
    {
        var referenced = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var id in LinkSourceIds())
            foreach (var link in ParsedLinks(id))
                if (link.Id != null && !string.Equals(link.Id, id, StringComparison.OrdinalIgnoreCase))
                    referenced.Add(link.Id);
        // только neurodoc-файлы: файлы в собственных форматах живут своей жизнью,
        // и «сирота» среди них — не повод для тревоги
        return _files.Keys
            .Where(id => id != RootId && IsNeurodocFile(id) && !referenced.Contains(id))
            .Select(id => new Backlink(id, GetTitle(id)))
            .OrderBy(b => b.Title, StringComparer.CurrentCultureIgnoreCase)
            .ToList();
    }

    /// <summary>Кратчайшие пути от root до указанного файла — их показывают хлебные крошки.</summary>
    /// <remarks>
    /// Сначала обход в ширину считает расстояния, и только потом перечисляются пути, где
    /// каждый шаг приближает к цели ровно на единицу. Прежний обход в глубину с отсечкой по
    /// числу найденных путей до короткого мог вообще не дойти: лимит выбирали длинные пути,
    /// найденные раньше (проверено — вместо пути длиной 3 показывался путь длиной 5).
    /// </remarks>
    public List<List<string>> FindShortestPathsFromRoot(string target, int maxPaths = 50)
    {
        var result = new List<List<string>>();
        if (string.Equals(target, RootId, StringComparison.OrdinalIgnoreCase)) return result;

        var graph = BuildGraph();
        var distance = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase) { [RootId] = 0 };
        var queue = new Queue<string>();
        queue.Enqueue(RootId);
        while (queue.Count > 0)
        {
            var current = queue.Dequeue();
            if (!graph.TryGetValue(current, out var next)) continue;
            foreach (var n in next)
            {
                if (distance.ContainsKey(n)) continue;
                distance[n] = distance[current] + 1;
                // дальше идут только файлы, которые сами бывают источниками ссылок;
                // контракт .yaml может быть целью, но своих исходящих у него нет
                if (graph.ContainsKey(n)) queue.Enqueue(n);
            }
        }
        if (!distance.TryGetValue(target, out int targetDistance)) return result;

        var path = new List<string> { RootId };

        void Walk(string node)
        {
            if (result.Count >= maxPaths) return;
            if (string.Equals(node, target, StringComparison.OrdinalIgnoreCase))
            {
                result.Add(new List<string>(path));
                return;
            }
            if (distance[node] >= targetDistance) return;
            if (!graph.TryGetValue(node, out var next)) return;
            foreach (var n in next)
            {
                if (!distance.TryGetValue(n, out int d) || d != distance[node] + 1) continue;
                path.Add(n);
                Walk(n);
                path.RemoveAt(path.Count - 1);
                if (result.Count >= maxPaths) return;
            }
        }

        Walk(RootId);
        result.Sort((a, b) => string.CompareOrdinal(string.Join('/', a), string.Join('/', b)));
        return result;
    }

    /// <summary>
    /// Порядок чтения всего каталога: обход в глубину от root по порядку ссылок в тексте
    /// (порядок ссылок — это замысел автора). Каждый файл встречается ровно один раз,
    /// циклы отсекаются множеством пройденных.
    /// </summary>
    /// <remarks>
    /// Хвостом идут файлы, до которых по ссылкам от root не добраться: без них «прочитать
    /// всё» было бы недостижимо. Порядок не кэшируется — документация меняется между
    /// сеансами, и новые файлы должны просто появляться в списке непрочитанными.
    /// </remarks>
    public List<string> BuildReadingOrder()
    {
        var order = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        // стек, а не рекурсия: глубина обхода равна длине цепочки ссылок
        var stack = new Stack<string>();
        stack.Push(RootId);
        while (stack.Count > 0)
        {
            var id = stack.Pop();
            if (!seen.Add(id)) continue;
            order.Add(id);

            // файлы в собственных форматах (контракты, конфиги) — справочные данные,
            // их читают по необходимости, а не подряд, поэтому в обход они не входят
            var children = new List<string>();
            foreach (var link in ParsedLinks(id))
            {
                var target = link.Id;
                if (target == null || !IsNeurodocFile(target) || !Exists(target)) continue;
                if (seen.Contains(target)) continue;
                if (!children.Contains(target, StringComparer.OrdinalIgnoreCase))
                    children.Add(target);
            }
            // в стек — в обратном порядке, чтобы первым читался первый по тексту
            for (int i = children.Count - 1; i >= 0; i--)
                stack.Push(children[i]);
        }

        foreach (var id in AllIds())
            if (IsNeurodocFile(id) && seen.Add(id))
                order.Add(id);

        return order;
    }

    /// <summary>Поиск подстроки по всем файлам проекта (без учёта регистра).</summary>
    public List<SearchResult> Search(string query, int maxResults = 200)
    {
        var results = new List<SearchResult>();
        if (string.IsNullOrEmpty(query)) return results;

        foreach (var id in AllIds())
        {
            var text = ReadFile(id);
            int index = 0, occurrence = 0;
            while (results.Count < maxResults &&
                   (index = text.IndexOf(query, index, StringComparison.OrdinalIgnoreCase)) >= 0)
            {
                int lineStart = index == 0 ? 0 : text.LastIndexOfAny(NewlineChars, index - 1) + 1;
                int matchEnd = index + query.Length;
                int lineEnd = text.IndexOfAny(NewlineChars, Math.Min(matchEnd, text.Length));
                if (lineEnd < 0) lineEnd = text.Length;

                int prefixFrom = Math.Max(lineStart, index - 60);
                var prefix = (prefixFrom > lineStart ? "…" : "") + text[prefixFrom..index];
                int suffixTo = Math.Min(lineEnd, matchEnd + 90);
                var suffix = text[matchEnd..suffixTo] + (suffixTo < lineEnd ? "…" : "");

                results.Add(new SearchResult(id, GetTitle(id, 40), prefix, text[index..matchEnd], suffix, occurrence));
                occurrence++;
                index += Math.Max(1, query.Length);
            }
            if (results.Count >= maxResults) break;
        }
        return results;
    }

    public const string ReadMeTemplate =
"""
# Каталог документации в формате neurodoc

Формат называется neurodoc.
Формат neurodoc представляет собой набор текстовых файлов.
Внутри каждого файла находится блок знаний или информации.
Все файлы в кодировке UTF-8.
Начинать читать надо с корневого файла - он всегда называется root.
Остальные файлы находятся в папке _content.
Их имена являются уникальными идентификаторами в рамках данного каталога.
Идентификаторы - это UUID в нижнем регистре.
В тексте любого файла может встретиться ссылка вида [текст](uuid). Например, вот так: [подход к разработке](5c673c53-8ad9-49eb-9fb6-ffb3954848f0).
Это значит, что это ссылка на другой файл в папке _content с именем 5c673c53-8ad9-49eb-9fb6-ffb3954848f0, а "подход к разработке" - её текст (якорь), встроенный в предложение.
Текст ссылки может быть пустым: [](5c673c53-8ad9-49eb-9fb6-ffb3954848f0). Тогда вместо него показывают заголовок целевого файла - его первую непустую строку.
Текст ссылки не содержит символа "]" и переводов строк.
UUID сам по себе ссылкой не считается. UUID, обрамлённый символами "<" и ">", - это ссылка старого формата, она больше ссылкой не считается.
Кроме папки _content в каталоге могут быть папки с файлами в собственных форматах (YAML, SVG, конфиги). Имена у них не UUID, а доменные: эти файлы читают не только люди, но и программы.
В ссылке вместо UUID может стоять путь к такому файлу - относительно того файла, в котором написана ссылка. Например, ссылка [auth.start](../contract/auth/start.yaml), написанная в файле из папки _content, ведёт на файл contract/auth/start.yaml этого же каталога.
Ссылка по пути ведёт только внутрь каталога neurodoc. За пределы каталога ссылок нет.
Таким образом, файлы образуют граф знаний, который имеет в качестве корня знания из файла root.
Для быстрой обработки каталога neurodoc рекомендуется построить в памяти hash-таблицу (mapping) "имя файла" -> "контент".
""";
}

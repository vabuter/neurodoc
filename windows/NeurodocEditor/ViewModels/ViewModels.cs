using Microsoft.UI.Xaml;

namespace NeurodocEditor.ViewModels;

public class BreadcrumbSegment
{
    public string Id { get; init; } = "";
    public string Title { get; init; } = "";
    public bool IsClickable { get; init; } = true;
    public Visibility ChevronVisibility { get; init; } = Visibility.Visible;
}

public class BreadcrumbPath
{
    public List<BreadcrumbSegment> Segments { get; init; } = new();
}

public class BacklinkVM
{
    public string Id { get; init; } = "";
    public string Title { get; init; } = "";
}

public class SearchResultVM
{
    public string FileId { get; init; } = "";
    public string Title { get; init; } = "";
    public string Prefix { get; init; } = "";
    public string Match { get; init; } = "";
    public string Suffix { get; init; } = "";
    public int Occurrence { get; init; }
}

public class ProjectItemVM
{
    public string Id { get; init; } = "";
    public string Name { get; init; } = "";
    public string Path { get; init; } = "";
    public Visibility CurrentBadgeVisibility { get; init; } = Visibility.Collapsed;
}

public class GitCommitVM
{
    public string Hash { get; init; } = "";
    /// <summary>Сообщение коммита — первая строка карточки.</summary>
    public string Message { get; init; } = "";
    /// <summary>Вторая строка: короткий хэш, дата, автор.</summary>
    public string Meta { get; init; } = "";
}

public class GitFileVM
{
    /// <summary>Путь файла в репозитории — им адресуется diff.</summary>
    public string Path { get; init; } = "";
    /// <summary>Идентификатор файла каталога, если файл открывается в редакторе.</summary>
    public string? FileId { get; init; }
    /// <summary>Заголовок файла; для UUID-имён — первая непустая строка содержимого.</summary>
    public string Title { get; init; } = "";
    /// <summary>Вторая строка: статус изменения и путь.</summary>
    public string Detail { get; init; } = "";
    public bool IsDeleted { get; init; }
}

public class BrokenLinkVM
{
    public string FileId { get; init; } = "";
    public string FileTitle { get; init; } = "";
    public string TargetId { get; init; } = "";
    public int Position { get; init; }
    public string Context { get; init; } = "";
}

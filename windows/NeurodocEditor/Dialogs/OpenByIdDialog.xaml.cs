using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using NeurodocEditor.Services;
using NeurodocEditor.ViewModels;
using Windows.ApplicationModel.DataTransfer;

namespace NeurodocEditor.Dialogs;

/// <summary>Переход к файлу по его UUID (или по выбору из списка файлов проекта).</summary>
public sealed partial class OpenByIdDialog : ContentDialog
{
    private const int MaxMatches = 200;

    private readonly NeurodocCatalog _catalog;
    private readonly List<BacklinkVM> _allFiles;
    private bool _suppressSync;

    /// <summary>Файл, который пользователь решил открыть; null — если отменил.</summary>
    public string? SelectedId { get; private set; }

    public OpenByIdDialog(NeurodocCatalog catalog)
    {
        _catalog = catalog;
        InitializeComponent();

        _allFiles = catalog.AllIds()
            .Select(id => new BacklinkVM { Id = id, Title = catalog.GetTitle(id) })
            .ToList();
        ApplyFilter("");
        Opened += OnDialogOpened;
    }

    private async void OnDialogOpened(ContentDialog sender, ContentDialogOpenedEventArgs args)
    {
        // если в буфере обмена лежит ссылка или UUID — подставляем сразу
        try
        {
            var data = Clipboard.GetContent();
            if (data.Contains(StandardDataFormats.Text))
            {
                var text = await data.GetTextAsync();
                if (NeurodocCatalog.TryParseId(text, out var id) && _catalog.Exists(id))
                    IdBox.Text = id;
            }
        }
        catch
        {
            // буфер обмена может быть занят другим процессом
        }
        IdBox.Focus(FocusState.Programmatic);
        IdBox.SelectAll();
    }

    private void OnIdChanged(object sender, TextChangedEventArgs e)
    {
        if (_suppressSync) return;
        ApplyFilter(IdBox.Text);
        UpdateStatus();
    }

    private void ApplyFilter(string query)
    {
        query = query.Trim().Trim('<', '>').Trim();
        var items = string.IsNullOrEmpty(query)
            ? _allFiles
            : _allFiles.Where(f =>
                f.Id.Contains(query, StringComparison.OrdinalIgnoreCase) ||
                f.Title.Contains(query, StringComparison.OrdinalIgnoreCase)).ToList();

        _suppressSync = true;
        MatchList.ItemsSource = items.Take(MaxMatches).ToList();
        _suppressSync = false;
        ListHeader.Text = items.Count == _allFiles.Count
            ? $"Файлы проекта: {_allFiles.Count}"
            : $"Найдено файлов: {items.Count}";
    }

    private void UpdateStatus()
    {
        var input = IdBox.Text;
        if (string.IsNullOrWhiteSpace(input))
        {
            StatusBar.IsOpen = false;
            return;
        }
        if (!_catalog.TryResolveInput(input, out var id))
        {
            // разобранный UUID, которого нет, и вовсе неразбираемый ввод — разные беды
            if (NeurodocCatalog.TryParseId(input, out var parsed))
                ShowStatus(InfoBarSeverity.Error, $"Файла {parsed} нет в этом проекте.");
            else
                ShowStatus(InfoBarSeverity.Warning, "Это не UUID и не путь к файлу проекта.");
            return;
        }
        ShowStatus(InfoBarSeverity.Success, _catalog.GetTitle(id));
    }

    private void ShowStatus(InfoBarSeverity severity, string message)
    {
        StatusBar.Severity = severity;
        StatusBar.Message = message;
        StatusBar.IsOpen = true;
    }

    private void OnMatchSelected(object sender, SelectionChangedEventArgs e)
    {
        if (_suppressSync || MatchList.SelectedItem is not BacklinkVM item) return;
        _suppressSync = true;
        IdBox.Text = item.Id;
        _suppressSync = false;
        UpdateStatus();
    }

    private void OnMatchDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (MatchList.SelectedItem is not BacklinkVM item) return;
        SelectedId = item.Id;
        Hide();
    }

    private void OnOpenClick(ContentDialog sender, ContentDialogButtonClickEventArgs args)
    {
        // не только UUID: в списке есть и файлы в собственных форматах, они
        // адресуются путём — выбранный там файл обязан открываться кнопкой,
        // а не одним лишь двойным щелчком
        if (_catalog.TryResolveInput(IdBox.Text, out var id))
        {
            SelectedId = id;
            return;
        }
        args.Cancel = true; // окно остаётся открытым
        UpdateStatus();
        if (string.IsNullOrWhiteSpace(IdBox.Text))
            ShowStatus(InfoBarSeverity.Warning, "Укажите файл или выберите его в списке.");
    }
}

using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using NeurodocEditor.Models;
using NeurodocEditor.Services;
using IOPath = System.IO.Path;

namespace NeurodocEditor.Dialogs;

/// <summary>
/// Клонирование репозитория с сервера в папку на диске. Непустую папку диалог
/// предупреждением предлагает очистить: клонировать можно только «как есть» с remote.
/// </summary>
public sealed partial class GitCloneDialog : ContentDialog
{
    /// <summary>Проект, созданный по успешному клонированию; null — отменено или не вышло.</summary>
    public ProjectInfo? CreatedProject { get; private set; }

    private bool _nameEdited;

    public GitCloneDialog()
    {
        InitializeComponent();
        NameBox.TextChanged += (_, _) =>
        {
            if (NameBox.FocusState != FocusState.Unfocused) _nameEdited = true;
        };
    }

    /// <summary>Название проекта подставляется из адреса, пока пользователь его не правил.</summary>
    private void OnUrlChanged(object sender, TextChangedEventArgs e)
    {
        if (_nameEdited) return;
        var url = UrlBox.Text.Trim().TrimEnd('/');
        var name = url[(url.LastIndexOf('/') + 1)..];
        if (name.EndsWith(".git", StringComparison.OrdinalIgnoreCase)) name = name[..^4];
        NameBox.Text = name;
    }

    private async void OnBrowseClick(object sender, RoutedEventArgs e)
    {
        try
        {
            var picker = new Windows.Storage.Pickers.FolderPicker
            {
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary,
            };
            picker.FileTypeFilter.Add("*");
            WinRT.Interop.InitializeWithWindow.Initialize(picker, MainWindow.Hwnd);
            var folder = await picker.PickSingleFolderAsync();
            if (folder != null)
                PathBox.Text = folder.Path;
        }
        catch (Exception ex)
        {
            ShowError("Не удалось открыть выбор папки: " + ex.Message);
        }
    }

    private void OnCloneClick(object sender, RoutedEventArgs e)
    {
        ErrorBar.IsOpen = false;
        var url = UrlBox.Text.Trim();
        var path = PathBox.Text.Trim();
        if (url.Length == 0)
        {
            ShowError("Укажите адрес репозитория.");
            return;
        }
        if (NameBox.Text.Trim().Length == 0)
        {
            ShowError("Укажите название проекта.");
            return;
        }
        if (path.Length == 0)
        {
            ShowError("Укажите папку на диске.");
            return;
        }
        try
        {
            path = IOPath.GetFullPath(path);
        }
        catch (Exception ex)
        {
            ShowError("Непонятный путь: " + ex.Message);
            return;
        }
        PathBox.Text = path;

        if (Directory.Exists(path) && Directory.EnumerateFileSystemEntries(path).Any())
        {
            ConfirmText.Text = $"В папке «{path}» уже есть файлы. Они будут удалены, " +
                               "и репозиторий будет клонирован с сервера как есть.";
            FormPane.Visibility = Visibility.Collapsed;
            ConfirmPane.Visibility = Visibility.Visible;
            return;
        }
        _ = CloneAsync(url, path);
    }

    private void OnBackClick(object sender, RoutedEventArgs e)
    {
        ConfirmPane.Visibility = Visibility.Collapsed;
        FormPane.Visibility = Visibility.Visible;
    }

    private void OnConfirmClick(object sender, RoutedEventArgs e)
    {
        OnBackClick(sender, e);
        _ = CloneAsync(UrlBox.Text.Trim(), PathBox.Text.Trim());
    }

    private async Task CloneAsync(string url, string path)
    {
        CloneButton.IsEnabled = false;
        IsEnabled = true;
        Busy.IsActive = true;
        try
        {
            await GitService.CloneAsync(url, path);
            CreatedProject = new ProjectInfo { Name = NameBox.Text.Trim(), Path = path };
            Hide();
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
        finally
        {
            Busy.IsActive = false;
            CloneButton.IsEnabled = true;
        }
    }

    private void ShowError(string message)
    {
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }
}

using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using NeurodocEditor.Models;
using NeurodocEditor.Services;
using NeurodocEditor.ViewModels;
using IOPath = System.IO.Path;

namespace NeurodocEditor.Dialogs;

public sealed partial class ProjectsDialog : ContentDialog
{
    private readonly AppSettings _settings;
    private readonly Action _save;
    private ProjectInfo? _editing;

    /// <summary>Проект, который пользователь решил открыть (кнопка «Открыть» или двойной клик).</summary>
    public ProjectInfo? ProjectToOpen { get; private set; }

    public ProjectsDialog(AppSettings settings, Action save)
    {
        _settings = settings;
        _save = save;
        InitializeComponent();
        RefreshList();
    }

    private ProjectItemVM? Selected => ProjectList.SelectedItem as ProjectItemVM;

    private void RefreshList()
    {
        var items = _settings.Projects.Select(p => new ProjectItemVM
        {
            Id = p.Id,
            Name = p.Name,
            Path = p.Path,
            CurrentBadgeVisibility = p.Id == _settings.CurrentProjectId
                ? Visibility.Visible
                : Visibility.Collapsed,
        }).ToList();
        ProjectList.ItemsSource = items;
        ProjectList.SelectedItem =
            items.FirstOrDefault(i => i.Id == _settings.CurrentProjectId) ?? items.FirstOrDefault();
        NoProjectsText.Visibility = items.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        UpdateButtons();
    }

    private void UpdateButtons()
    {
        bool has = Selected != null;
        EditButton.IsEnabled = has;
        DeleteButton.IsEnabled = has;
        IsPrimaryButtonEnabled = has && EditPane.Visibility == Visibility.Collapsed;
    }

    private void OnSelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateButtons();

    private void OnItemDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (Selected == null) return;
        CommitOpen();
        Hide();
    }

    private void OnOpenClick(ContentDialog sender, ContentDialogButtonClickEventArgs args)
    {
        if (Selected == null)
        {
            args.Cancel = true;
            return;
        }
        CommitOpen();
    }

    private void CommitOpen()
    {
        var project = _settings.Projects.FirstOrDefault(p => p.Id == Selected!.Id);
        if (project == null) return;
        _settings.CurrentProjectId = project.Id;
        _save();
        ProjectToOpen = project;
    }

    // ---------- Добавление / изменение ----------

    private void OnAddClick(object sender, RoutedEventArgs e) => ShowEdit(null);

    private void OnEditClick(object sender, RoutedEventArgs e)
    {
        if (Selected == null) return;
        ShowEdit(_settings.Projects.FirstOrDefault(p => p.Id == Selected.Id));
    }

    private void ShowEdit(ProjectInfo? project)
    {
        _editing = project;
        EditPaneTitle.Text = project == null ? "Новый проект" : "Изменить проект";
        NameBox.Text = project?.Name ?? "";
        PathBox.Text = project?.Path ?? "";
        EditError.IsOpen = false;
        ListPane.Visibility = Visibility.Collapsed;
        EditPane.Visibility = Visibility.Visible;
        PrimaryButtonText = "";
        CloseButtonText = "";
        IsPrimaryButtonEnabled = false;
    }

    private void HideEdit()
    {
        ListPane.Visibility = Visibility.Visible;
        EditPane.Visibility = Visibility.Collapsed;
        PrimaryButtonText = "Открыть";
        CloseButtonText = "Закрыть";
        UpdateButtons();
    }

    private void OnCancelEditClick(object sender, RoutedEventArgs e) => HideEdit();

    private void OnSaveEditClick(object sender, RoutedEventArgs e)
    {
        var name = NameBox.Text.Trim();
        var path = PathBox.Text.Trim();
        if (name.Length == 0)
        {
            ShowEditError("Укажите название проекта.");
            return;
        }
        if (path.Length == 0)
        {
            ShowEditError("Укажите путь к каталогу проекта.");
            return;
        }
        try
        {
            path = IOPath.GetFullPath(path);
            NeurodocCatalog.EnsureStructure(path);
        }
        catch (Exception ex)
        {
            ShowEditError("Не удалось создать структуру каталога: " + ex.Message);
            return;
        }

        if (_editing == null)
        {
            var project = new ProjectInfo { Name = name, Path = path };
            _settings.Projects.Add(project);
            _settings.CurrentProjectId ??= project.Id;
        }
        else
        {
            _editing.Name = name;
            _editing.Path = path;
        }
        _save();
        RefreshList();
        HideEdit();
    }

    private void ShowEditError(string message)
    {
        EditError.Message = message;
        EditError.IsOpen = true;
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
            ShowEditError("Не удалось открыть выбор папки: " + ex.Message);
        }
    }

    // ---------- Удаление ----------

    private void OnDeleteConfirm(object sender, RoutedEventArgs e)
    {
        DeleteFlyout.Hide();
        if (Selected == null) return;
        var project = _settings.Projects.FirstOrDefault(p => p.Id == Selected.Id);
        if (project == null) return;
        _settings.Projects.Remove(project);
        if (_settings.CurrentProjectId == project.Id)
            _settings.CurrentProjectId = _settings.Projects.FirstOrDefault()?.Id;
        _save();
        RefreshList();
    }
}

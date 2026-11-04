using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using NeurodocEditor.Services;
using NeurodocEditor.ViewModels;

namespace NeurodocEditor;

/// <summary>
/// Git: сохранение и получение изменений, ветки, история, дельты. Каждый commit сразу
/// уходит push — локальные коммиты приложению не нужны. Конфликты решаются перезатиранием
/// по явному выбору пользователя либо отменой.
/// </summary>
public sealed partial class MainWindow
{
    private GitService? _git;
    private bool _gitBusy;
    /// <summary>Есть ли git в PATH; уточняется при открытии проекта.</summary>
    private bool _gitAvailable = true;
    private string? _gitBranch;
    /// <summary>Кэш веток для меню: Opening у MenuFlyout синхронный, спросить git из него нельзя.</summary>
    private List<string> _gitBranches = new();
    private string? _gitDefaultBranch;

    private List<GitCommit> _gitCommits = new();
    private GitCommit? _gitSelectedCommit;

    // ================= Состояние и заголовок =================

    /// <summary>Пересоздаёт сервис при открытии проекта. Зовётся из OpenProject.</summary>
    private void SetupGit(string? projectPath)
    {
        _git = projectPath != null && GitService.IsRepository(projectPath)
            ? new GitService(projectPath)
            : null;
        _gitCommits = new();
        _gitSelectedCommit = null;
        GitHistoryCard.Visibility = Visibility.Collapsed;
        _ = RefreshGitUiAsync();
    }

    /// <summary>Обновляет ветку в заголовке и кэш веток. После каждой git-операции.</summary>
    private async Task RefreshGitUiAsync()
    {
        _gitAvailable = await GitService.IsGitAvailableAsync();
        if (_git == null || !_gitAvailable)
        {
            _gitBranch = null;
            _gitBranches = new();
            _gitDefaultBranch = null;
            TitleBranchText.Text = "";
            return;
        }
        try
        {
            _gitBranch = await _git.GetCurrentBranchAsync();
            _gitBranches = await _git.GetLocalBranchesAsync();
            _gitDefaultBranch = await _git.GetDefaultBranchAsync();
        }
        catch (GitException)
        {
            _gitBranch = null;
            _gitBranches = new();
        }
        TitleBranchText.Text = _gitBranch == null ? "" : "·  " + _gitBranch;
        if (GitHistoryCard.Visibility == Visibility.Visible)
            await LoadGitCommitsAsync();
    }

    // ================= Меню =================

    /// <summary>Меню строится при каждом открытии: набор пунктов зависит от состояния репозитория.</summary>
    private void OnGitFlyoutOpening(object sender, object e)
    {
        var items = GitFlyout.Items;
        items.Clear();

        if (!_gitAvailable)
        {
            items.Add(new MenuFlyoutItem
            {
                Text = "git не найден — установите Git for Windows",
                IsEnabled = false,
            });
            return;
        }

        bool repo = _git != null && !_gitBusy;

        items.Add(MakeGitItem("Сохранить на сервере", repo, OnGitSaveClick));
        items.Add(MakeGitItem("Получить с сервера", repo, OnGitPullClick));
        items.Add(MakeGitItem("Синхронизировать", repo, OnGitSyncClick));
        items.Add(new MenuFlyoutSeparator());

        var branches = new MenuFlyoutSubItem { Text = "Ветки", IsEnabled = repo };
        foreach (var branch in _gitBranches)
        {
            var name = branch;
            var item = new RadioMenuFlyoutItem { Text = name, IsChecked = name == _gitBranch };
            item.Click += (_, _) => _ = GitSwitchBranchAsync(name);
            branches.Items.Add(item);
        }
        if (_gitBranches.Count > 0) branches.Items.Add(new MenuFlyoutSeparator());
        var create = new MenuFlyoutItem { Text = "Создать ветку…" };
        create.Click += (_, _) => _ = GitCreateBranchAsync();
        branches.Items.Add(create);
        var deleteSub = new MenuFlyoutSubItem { Text = "Удалить ветку" };
        foreach (var branch in _gitBranches.Where(b => b != _gitDefaultBranch))
        {
            var name = branch;
            var item = new MenuFlyoutItem { Text = name };
            item.Click += (_, _) => _ = GitDeleteBranchAsync(name);
            deleteSub.Items.Add(item);
        }
        deleteSub.IsEnabled = deleteSub.Items.Count > 0;
        branches.Items.Add(deleteSub);
        items.Add(branches);

        items.Add(MakeGitItem("История коммитов", repo, OnGitHistoryClick));
        items.Add(new MenuFlyoutSeparator());

        if (_catalog != null && _git == null)
            items.Add(MakeGitItem("Опубликовать проект на сервере…", !_gitBusy, OnGitPublishClick));
        items.Add(MakeGitItem("Клонировать с сервера…", !_gitBusy, OnGitCloneClick));
    }

    private static MenuFlyoutItem MakeGitItem(string text, bool enabled, RoutedEventHandler click)
    {
        var item = new MenuFlyoutItem { Text = text, IsEnabled = enabled };
        item.Click += click;
        return item;
    }

    // ================= Общий раннер операций =================

    /// <summary>
    /// Каркас каждой git-операции: сохранить правки, показать занятость, выполнить,
    /// перечитать каталог, обновить ветку. Ошибки git всегда показываются пользователю.
    /// </summary>
    private async Task RunGitAsync(string errorTitle, Func<GitService, Task> action)
    {
        if (_git == null || _gitBusy) return;
        SaveCurrentFile();
        _gitBusy = true;
        GitProgress.IsActive = true;
        try
        {
            await action(_git);
        }
        catch (Exception ex)
        {
            await ShowErrorAsync(errorTitle, ex.Message);
        }
        finally
        {
            _gitBusy = false;
            GitProgress.IsActive = false;
        }
        ReloadFromDisk(manual: false);
        EnsureCurrentFileExists();
        await RefreshGitUiAsync();
    }

    /// <summary>После pull/checkout/reset текущий файл мог исчезнуть — возвращаемся в root.</summary>
    private void EnsureCurrentFileExists()
    {
        if (_catalog == null || _currentId == null || _catalog.Exists(_currentId)) return;
        _currentId = null;
        OpenFile(NeurodocCatalog.RootId, push: false);
        ShowCopyTip(GitButton, "Файл исчез после операции git", "Открыт корневой файл");
    }

    // ================= Истории 5–7: сохранить, получить, синхронизировать =================

    private async void OnGitSaveClick(object sender, RoutedEventArgs e) =>
        await RunGitAsync("Не удалось сохранить на сервере", async git =>
        {
            var outcome = await git.CommitAndPushAsync(GitService.BuildCommitMessage());
            if (outcome == PushOutcome.RejectedNonFastForward)
                await PromptForcePushAsync(git);
            else
                ShowCopyTip(GitButton, "Сохранено на сервере", "Ветка " + (_gitBranch ?? "?"));
        });

    private async void OnGitPullClick(object sender, RoutedEventArgs e) =>
        await RunGitAsync("Не удалось получить с сервера", async git =>
        {
            var outcome = await git.PullAsync();
            if (outcome == PullOutcome.Conflict)
                await PromptTakeServerAsync(git);
            else
                ShowCopyTip(GitButton, "Получено с сервера", "Ветка " + (_gitBranch ?? "?"));
        });

    private async void OnGitSyncClick(object sender, RoutedEventArgs e) =>
        await RunGitAsync("Не удалось синхронизировать", async git =>
        {
            var outcome = await git.SyncAsync(GitService.BuildCommitMessage());
            switch (outcome)
            {
                case SyncOutcome.Conflict:
                    await PromptSyncConflictAsync(git);
                    break;
                case SyncOutcome.PushRejected:
                    await PromptForcePushAsync(git);
                    break;
                default:
                    ShowCopyTip(GitButton, "Синхронизировано", "Ветка " + (_gitBranch ?? "?"));
                    break;
            }
        });

    // ================= Диалоги конфликтов =================

    private async Task PromptForcePushAsync(GitService git)
    {
        var ask = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = "Конфликт с сервером",
            Content = "На сервере есть изменения, которых нет у вас.\n\n" +
                      "Можно перезаписать серверную версию своей — серверные изменения " +
                      "будут потеряны. Либо отмените операцию и разрешите конфликт в " +
                      "специализированном git-приложении.",
            PrimaryButtonText = "Мои затирают серверные",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await ask.ShowAsync() != ContentDialogResult.Primary) return;
        await git.ForcePushAsync();
        ShowCopyTip(GitButton, "Сохранено на сервере", "Серверная версия перезаписана");
    }

    private async Task PromptTakeServerAsync(GitService git)
    {
        var ask = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = "Конфликт с сервером",
            Content = "Серверные изменения конфликтуют с вашими.\n\n" +
                      "Можно взять серверную версию — ваши локальные изменения будут " +
                      "потеряны. Либо отмените операцию и разрешите конфликт в " +
                      "специализированном git-приложении.",
            PrimaryButtonText = "Серверные затирают мои",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await ask.ShowAsync() != ContentDialogResult.Primary) return;
        await git.ResetToRemoteAsync();
        ShowCopyTip(GitButton, "Получено с сервера", "Локальная версия перезаписана");
    }

    private async Task PromptSyncConflictAsync(GitService git)
    {
        var ask = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = "Конфликт с сервером",
            Content = "Ваши изменения и серверные конфликтуют.\n\n" +
                      "«Взять серверные» — ваши изменения будут потеряны.\n" +
                      "«Взять мои» — серверные изменения будут потеряны.\n" +
                      "Либо отмените операцию и разрешите конфликт в специализированном " +
                      "git-приложении.",
            PrimaryButtonText = "Взять серверные",
            SecondaryButtonText = "Взять мои",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Close,
        };
        switch (await ask.ShowAsync())
        {
            case ContentDialogResult.Primary:
                await git.ResetToRemoteAsync();
                ShowCopyTip(GitButton, "Синхронизировано", "Взята серверная версия");
                break;
            case ContentDialogResult.Secondary:
                await git.ForcePushAsync();
                ShowCopyTip(GitButton, "Синхронизировано", "Взята ваша версия");
                break;
        }
    }

    // ================= Истории 3–4: ветки =================

    private Task GitSwitchBranchAsync(string branch) =>
        RunGitAsync("Не удалось переключить ветку", async git =>
        {
            await git.SwitchBranchAsync(branch);
            ShowCopyTip(GitButton, "Ветка переключена", branch);
        });

    private async Task GitCreateBranchAsync()
    {
        var name = await PromptTextAsync("Создать ветку", "Имя ветки",
            "например, feature-auth");
        if (string.IsNullOrWhiteSpace(name)) return;
        name = name.Trim();
        if (name.Any(char.IsWhiteSpace) || name.IndexOfAny(new[] { '~', '^', ':', '?', '*', '[', '\\' }) >= 0)
        {
            await ShowErrorAsync("Создать ветку",
                "Имя ветки не должно содержать пробелов и символов ~ ^ : ? * [ \\");
            return;
        }
        await RunGitAsync("Не удалось создать ветку", async git =>
        {
            await git.CreateBranchAsync(name);
            ShowCopyTip(GitButton, "Ветка создана и отправлена на сервер", name);
        });
    }

    private async Task GitDeleteBranchAsync(string name)
    {
        var ask = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = "Удалить ветку?",
            Content = $"Ветка «{name}» будет удалена и локально, и на сервере.",
            PrimaryButtonText = "Удалить",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await ask.ShowAsync() != ContentDialogResult.Primary) return;

        await RunGitAsync("Не удалось удалить ветку", async git =>
        {
            // текущую ветку сначала покидаем — уходим на default
            if (name == _gitBranch)
                await git.SwitchBranchAsync(await git.GetDefaultBranchAsync());
            await git.DeleteBranchAsync(name);
            ShowCopyTip(GitButton, "Ветка удалена", name);
        });
    }

    // ================= История 1: опубликовать =================

    private async void OnGitPublishClick(object sender, RoutedEventArgs e)
    {
        if (_catalog == null) return;
        if (GitService.IsRepository(_catalog.RootPath))
        {
            await ShowErrorAsync("Опубликовать проект", "Каталог уже является git-репозиторием.");
            return;
        }
        var url = await PromptTextAsync("Опубликовать проект на сервере",
            "Адрес пустого репозитория (HTTPS)", "https://gitlab.com/you/project.git");
        if (string.IsNullOrWhiteSpace(url)) return;

        // публикация делает каталог репозиторием — пересоздаём сервис до раннера
        _git = new GitService(_catalog.RootPath);
        await RunGitAsync("Не удалось опубликовать проект", async git =>
        {
            try
            {
                await git.InitAndPublishAsync(url.Trim());
                ShowCopyTip(GitButton, "Проект опубликован", url.Trim());
            }
            catch
            {
                // репозиторий не состоялся — не притворяемся, что он есть
                if (!GitService.IsRepository(git.WorkDir)) _git = null;
                throw;
            }
        });
    }

    // ================= История 2: клонировать =================

    private async void OnGitCloneClick(object sender, RoutedEventArgs e)
    {
        var dialog = new Dialogs.GitCloneDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
        };
        await dialog.ShowAsync();
        if (dialog.CreatedProject is not { } project) return;

        App.Settings.Projects.Add(project);
        App.Settings.CurrentProjectId = project.Id;
        App.SettingsService.Save(App.Settings);
        OpenProject(project);
    }

    // ================= Истории 8–9: история и дельты =================

    private async void OnGitHistoryClick(object sender, RoutedEventArgs e)
    {
        GitHistoryCard.Visibility = Visibility.Visible;
        ShowGitCommitsPane();
        await LoadGitCommitsAsync();
    }

    private void OnGitHistoryCloseClick(object sender, RoutedEventArgs e) =>
        GitHistoryCard.Visibility = Visibility.Collapsed;

    private void OnGitBackToCommitsClick(object sender, RoutedEventArgs e) => ShowGitCommitsPane();

    /// <summary>Режим карточки: список коммитов.</summary>
    private void ShowGitCommitsPane()
    {
        _gitSelectedCommit = null;
        GitHistoryHeader.Text = "История · " + (_gitBranch ?? "?");
        GitBackToCommitsButton.Visibility = Visibility.Collapsed;
        GitSinceToggle.Visibility = Visibility.Collapsed;
        GitFilesList.Visibility = Visibility.Collapsed;
        GitCommitsList.Visibility = Visibility.Visible;
    }

    private async Task LoadGitCommitsAsync()
    {
        if (_git == null) return;
        try
        {
            _gitCommits = await _git.GetHistoryAsync();
        }
        catch (Exception ex)
        {
            await ShowErrorAsync("История коммитов", ex.Message);
            return;
        }
        GitHistoryHeader.Text = "История · " + (_gitBranch ?? "?");
        GitCommitsList.ItemsSource = _gitCommits.Select(c => new GitCommitVM
        {
            Hash = c.Hash,
            Message = c.Message,
            Meta = $"{c.ShortHash}  {c.Date.ToLocalTime():dd.MM.yyyy HH:mm}  {c.Author}",
        }).ToList();
        GitHistoryEmptyText.Visibility =
            _gitCommits.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        // выбранный коммит после перечитывания мог остаться — возвращаемся к списку
        if (_gitSelectedCommit != null &&
            _gitCommits.All(c => c.Hash != _gitSelectedCommit.Hash))
            ShowGitCommitsPane();
    }

    private async void OnGitCommitClick(object sender, ItemClickEventArgs e)
    {
        if (_git == null || e.ClickedItem is not GitCommitVM vm) return;
        _gitSelectedCommit = _gitCommits.FirstOrDefault(c => c.Hash == vm.Hash);
        if (_gitSelectedCommit == null) return;
        GitSinceToggle.IsChecked = false;
        await LoadGitFilesAsync();
    }

    private async void OnGitSinceToggled(object sender, RoutedEventArgs e) => await LoadGitFilesAsync();

    /// <summary>Режим карточки: файлы выбранного коммита либо всё изменённое с него.</summary>
    private async Task LoadGitFilesAsync()
    {
        if (_git == null || _catalog == null || _gitSelectedCommit is not { } commit) return;
        bool since = GitSinceToggle.IsChecked == true;
        List<GitFileChange> changes;
        try
        {
            changes = since
                ? await _git.GetChangesSinceAsync(commit.Hash)
                : await _git.GetCommitFilesAsync(commit.Hash);
        }
        catch (Exception ex)
        {
            await ShowErrorAsync("Изменённые файлы", ex.Message);
            return;
        }

        var items = new List<GitFileVM>();
        foreach (var change in changes)
        {
            if (IsServiceFile(change.Path)) continue;
            items.Add(await BuildGitFileVMAsync(commit, change, since));
        }

        GitHistoryHeader.Text = (since ? "С коммита " : "Коммит ") + commit.ShortHash;
        GitBackToCommitsButton.Visibility = Visibility.Visible;
        GitSinceToggle.Visibility = Visibility.Visible;
        GitCommitsList.Visibility = Visibility.Collapsed;
        GitFilesList.Visibility = Visibility.Visible;
        GitFilesList.ItemsSource = items;
        GitHistoryEmptyText.Visibility = items.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>Служебные файлы репозитория в списке изменений не показываются.</summary>
    private static bool IsServiceFile(string path)
    {
        var name = path[(path.LastIndexOf('/') + 1)..];
        return name.StartsWith('.');
    }

    private async Task<GitFileVM> BuildGitFileVMAsync(GitCommit commit, GitFileChange change, bool since)
    {
        // путь в репозитории -> идентификатор файла каталога
        string? id = change.Path == NeurodocCatalog.RootId
            ? NeurodocCatalog.RootId
            : NeurodocCatalog.TryResolveLink(NeurodocCatalog.RootId, change.Path, out var resolved)
                ? resolved
                : null;

        bool deleted = change.Status == 'D';
        string title;
        if (!deleted && id != null && _catalog!.Exists(id))
        {
            title = _catalog.GetTitle(id);
        }
        else
        {
            // файла уже нет — заголовок берём из прежней ревизии
            var rev = since ? commit.Hash : commit.Hash + "^";
            var text = _git == null ? null : await _git.ReadBlobAsync(rev, change.OldPath ?? change.Path);
            title = FirstNonEmptyLine(text) ?? change.Path;
        }

        string status = change.Status switch
        {
            'A' => "добавлен",
            'D' => "удалён",
            'R' => "переименован",
            _ => "изменён",
        };
        return new GitFileVM
        {
            Path = change.Path,
            FileId = id,
            Title = title,
            Detail = status + "  " + change.Path,
            IsDeleted = deleted,
        };
    }

    private static string? FirstNonEmptyLine(string? text)
    {
        if (text == null) return null;
        foreach (var raw in text.Split('\n'))
        {
            var line = raw.Trim().TrimStart('#').Trim();
            if (line.Length > 0) return line.Length > 60 ? line[..60] : line;
        }
        return null;
    }

    /// <summary>Клик по файлу — открыть его в редакторе; дельту показывает кнопка «±».</summary>
    private void OnGitFileClick(object sender, ItemClickEventArgs e)
    {
        if (_catalog == null || e.ClickedItem is not GitFileVM vm) return;
        if (vm.IsDeleted || vm.FileId == null || !_catalog.Exists(vm.FileId))
        {
            ShowCopyTip(GitButton, "Файла больше нет в каталоге", vm.Path);
            return;
        }
        OpenFile(vm.FileId);
    }

    private async void OnGitDiffClick(object sender, RoutedEventArgs e)
    {
        if (_git == null || _gitSelectedCommit is not { } commit ||
            (sender as FrameworkElement)?.Tag is not GitFileVM vm) return;
        try
        {
            bool since = GitSinceToggle.IsChecked == true;
            var diff = await _git.GetFileDiffAsync(commit.Hash, vm.Path, since);
            await ShowDiffAsync(commit.Hash + (since ? "~" : "·") + vm.Path, vm.Title, diff);
        }
        catch (Exception ex)
        {
            await ShowErrorAsync("Дельта файла", ex.Message);
        }
    }

    // ================= Вспомогательное =================

    /// <summary>Диалог с одним текстовым полем; null — пользователь отменил.</summary>
    private async Task<string?> PromptTextAsync(string title, string header, string placeholder)
    {
        var box = new TextBox { Header = header, PlaceholderText = placeholder };
        var ask = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = title,
            Content = box,
            PrimaryButtonText = "OK",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Primary,
        };
        return await ask.ShowAsync() == ContentDialogResult.Primary ? box.Text : null;
    }
}

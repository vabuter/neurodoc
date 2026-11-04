using System.Text.RegularExpressions;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using NeurodocEditor.Dialogs;
using NeurodocEditor.Services;

namespace NeurodocEditor;

/// <summary>Поиск по текущему файлу (панель Ctrl+F) и «Поиск и замена» (Ctrl+R).</summary>
public sealed partial class MainWindow
{
    // ================= Панель поиска по файлу =================

    private void OnFindInFileAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        ShowFindBar();
        args.Handled = true;
    }

    private void ShowFindBar()
    {
        if (_catalog == null || _currentId == null) return;
        ResetViewMode();
        FindBar.Visibility = Visibility.Visible;
        FindBox.Focus(FocusState.Programmatic);
        FindBox.SelectAll();
        UpdateFindCount();
    }

    private void CloseFindBar()
    {
        FindBar.Visibility = Visibility.Collapsed;
        ClearFindHighlights();
        Editor.Focus(FocusState.Programmatic);
    }

    private void OnSearchBoxKeyDown(object sender, KeyRoutedEventArgs e)
    {
        // Esc в поиске по проекту — вернуть фокус в редактор
        if (e.Key == Windows.System.VirtualKey.Escape)
        {
            Editor.Focus(FocusState.Programmatic);
            e.Handled = true;
        }
    }

    private void OnFindCloseClick(object sender, RoutedEventArgs e) => CloseFindBar();

    private void OnFindNextClick(object sender, RoutedEventArgs e) => FindStep(1);

    private void OnFindPrevClick(object sender, RoutedEventArgs e) => FindStep(-1);

    private void OnFindBoxKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            FindStep(IsKeyDown(Windows.System.VirtualKey.Shift) ? -1 : 1);
            e.Handled = true;
        }
        else if (e.Key == Windows.System.VirtualKey.Escape)
        {
            CloseFindBar();
            e.Handled = true;
        }
    }

    private void OnFindBoxTextChanged(object sender, TextChangedEventArgs e) => FindStep(0);

    /// <summary>Поиск в текущем файле: 1 — вперёд, -1 — назад, 0 — от текущей позиции включительно.</summary>
    private void FindStep(int direction)
    {
        var query = FindBox.Text;
        if (string.IsNullOrEmpty(query))
        {
            FindCountText.Text = "";
            return;
        }

        var text = GetEditorTextRaw();
        var selection = Editor.Document.Selection;
        int selStart = Math.Clamp(Math.Min(selection.StartPosition, selection.EndPosition), 0, text.Length);
        int selEnd = Math.Clamp(Math.Max(selection.StartPosition, selection.EndPosition), 0, text.Length);

        int index = -1;
        if (direction >= 0)
        {
            int from = Math.Min(direction > 0 ? selEnd : selStart, text.Length);
            index = text.IndexOf(query, from, StringComparison.OrdinalIgnoreCase);
            if (index < 0)
                index = text.IndexOf(query, 0, StringComparison.OrdinalIgnoreCase); // с начала
        }
        else
        {
            int from = selStart - 1;
            if (from >= 0 && text.Length > 0)
                index = text.LastIndexOf(query, Math.Min(from, text.Length - 1), StringComparison.OrdinalIgnoreCase);
            if (index < 0 && text.Length > 0)
                index = text.LastIndexOf(query, text.Length - 1, StringComparison.OrdinalIgnoreCase); // с конца
        }

        if (index < 0)
        {
            FindCountText.Text = "0";
            return;
        }

        selection.SetRange(index, index + query.Length);
        EnsureCaretVisible();
        UpdateFindCount(index);
    }

    private void UpdateFindCount(int currentIndex = -1)
    {
        var query = FindBox.Text;
        _findMatches.Clear();
        _findCurrentIndex = currentIndex;
        if (string.IsNullOrEmpty(query))
        {
            FindCountText.Text = "";
            RedrawOverlay();
            return;
        }
        var text = GetEditorTextRaw();
        int total = 0, current = 0, index = 0;
        while ((index = text.IndexOf(query, index, StringComparison.OrdinalIgnoreCase)) >= 0)
        {
            total++;
            if (index == currentIndex) current = total;
            _findMatches.Add((index, index + query.Length));
            index += Math.Max(1, query.Length);
        }
        FindCountText.Text = current > 0 ? $"{current} из {total}" : total.ToString();
        RedrawOverlay(); // перерисовать подсветку совпадений
    }

    private readonly List<(int Start, int End)> _findMatches = new();
    private int _findCurrentIndex = -1;

    private void ClearFindHighlights()
    {
        _findMatches.Clear();
        _findCurrentIndex = -1;
        RedrawOverlay();
    }

    /// <summary>
    /// Подсветка совпадений поиска на оверлее: RichEdit прячет своё выделение,
    /// пока фокус в поле поиска, поэтому рисуем сами (жёлтым, текущее — с рамкой).
    /// </summary>
    private void DrawFindHighlights()
    {
        if (_findMatches.Count == 0 || FindBar.Visibility != Visibility.Visible) return;
        var doc = Editor.Document;
        double offsetX = Editor.Padding.Left;
        double offsetY = Editor.Padding.Top;
        bool dark = RootGrid.ActualTheme == Microsoft.UI.Xaml.ElementTheme.Dark;
        var fill = Windows.UI.Color.FromArgb(dark ? (byte)0x46 : (byte)0x5A, 255, 193, 7);
        var currentStroke = Windows.UI.Color.FromArgb(0xE6, 255, 152, 0);

        foreach (var (start, end) in _findMatches)
        {
            var range = doc.GetRange(start, end);
            range.GetRect(PointOptions.AllowOffClient, out var rect, out _);
            if (rect.Width <= 0 || rect.Height <= 0) continue;

            var shape = new Microsoft.UI.Xaml.Shapes.Rectangle
            {
                Width = rect.Width + 4,
                Height = rect.Height + 2,
                RadiusX = 3,
                RadiusY = 3,
                IsHitTestVisible = false,
                Fill = new Microsoft.UI.Xaml.Media.SolidColorBrush(fill),
            };
            if (start == _findCurrentIndex)
            {
                shape.Stroke = new Microsoft.UI.Xaml.Media.SolidColorBrush(currentStroke);
                shape.StrokeThickness = 1.5;
            }
            Canvas.SetLeft(shape, rect.X + offsetX - 2);
            Canvas.SetTop(shape, rect.Y + offsetY - 1);
            LinkOverlay.Children.Add(shape);
        }
    }

    // ================= Поиск и замена =================

    private void OnReplaceClick(object sender, RoutedEventArgs e) =>
        _ = ShowReplaceDialogAsync(wholeProjectScope: false);

    private void OnReplaceAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        _ = ShowReplaceDialogAsync(wholeProjectScope: false);
        args.Handled = true;
    }

    private void OnReplaceProjectAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        _ = ShowReplaceDialogAsync(wholeProjectScope: true);
        args.Handled = true;
    }

    private bool _replaceDialogOpen;

    private async Task ShowReplaceDialogAsync(bool wholeProjectScope)
    {
        if (_replaceDialogOpen || _catalog == null || _currentId == null) return;
        _replaceDialogOpen = true;
        try
        {
            await ShowReplaceDialogCoreAsync(wholeProjectScope);
        }
        finally
        {
            _replaceDialogOpen = false;
        }
    }

    private async Task ShowReplaceDialogCoreAsync(bool wholeProjectScope)
    {
        ResetViewMode();
        var dialog = new ReplaceDialog(
            replaceOne: (query, replacement, wholeProject) =>
                wholeProject ? ReplaceOneInProject(query, replacement) : ReplaceOneInCurrentFile(query, replacement),
            replaceAll: (query, replacement, wholeProject) =>
                wholeProject ? ReplaceAllInProject(query, replacement) : ReplaceAllInCurrentFile(query, replacement),
            wholeProjectScope)
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
        };
        try { await dialog.ShowAsync(); } catch { }
    }

    private static Regex BuildFindRegex(string query) =>
        new(Regex.Escape(query), RegexOptions.IgnoreCase);

    /// <summary>Заменяет одно вхождение (от каретки, с переходом по кругу). true — заменено.</summary>
    private bool ReplaceOneInCurrentFile(string query, string replacement)
    {
        if (_catalog == null || _currentId == null || string.IsNullOrEmpty(query)) return false;
        var text = GetEditorTextRaw();
        var selection = Editor.Document.Selection;
        int from = Math.Clamp(Math.Min(selection.StartPosition, selection.EndPosition), 0, text.Length);
        int index = text.IndexOf(query, from, StringComparison.OrdinalIgnoreCase);
        if (index < 0)
            index = text.IndexOf(query, 0, StringComparison.OrdinalIgnoreCase);
        if (index < 0) return false;

        Editor.Document.GetRange(index, index + query.Length)
            .SetText(TextSetOptions.None, replacement);
        selection.SetRange(index, index + replacement.Length);
        EnsureCaretVisible();
        SaveCurrentFile();
        return true;
    }

    /// <summary>Заменяет все вхождения в текущем файле одним шагом Undo. Возвращает число замен.</summary>
    private int ReplaceAllInCurrentFile(string query, string replacement)
    {
        if (_catalog == null || _currentId == null || string.IsNullOrEmpty(query)) return 0;
        var text = GetEditorTextRaw();
        var regex = BuildFindRegex(query);
        int count = regex.Matches(text).Count;
        if (count == 0) return 0;

        var replaced = regex.Replace(text, replacement.Replace("$", "$$"));
        Editor.Document.GetRange(0, text.Length + 1).SetText(TextSetOptions.None, replaced);
        Editor.Document.Selection.SetRange(0, 0);
        SaveCurrentFile();
        return count;
    }

    private bool ReplaceOneInProject(string query, string replacement)
    {
        if (_catalog == null || string.IsNullOrEmpty(query)) return false;
        if (ReplaceOneInCurrentFile(query, replacement)) return true;

        foreach (var id in _catalog.AllIds().ToList())
        {
            if (string.Equals(id, _currentId, StringComparison.OrdinalIgnoreCase)) continue;
            if (_catalog.ReadFile(id).IndexOf(query, StringComparison.OrdinalIgnoreCase) < 0) continue;

            OpenFile(id);
            var text = GetEditorTextRaw();
            int index = text.IndexOf(query, StringComparison.OrdinalIgnoreCase);
            if (index < 0) return false;
            Editor.Document.GetRange(index, index + query.Length)
                .SetText(TextSetOptions.None, replacement);
            Editor.Document.Selection.SetRange(index, index + replacement.Length);
            EnsureCaretVisible();
            SaveCurrentFile();
            return true;
        }
        return false;
    }

    private int ReplaceAllInProject(string query, string replacement)
    {
        if (_catalog == null || string.IsNullOrEmpty(query)) return 0;
        var regex = BuildFindRegex(query);
        var safeReplacement = replacement.Replace("$", "$$");
        int total = 0;

        foreach (var id in _catalog.AllIds().ToList())
        {
            if (string.Equals(id, _currentId, StringComparison.OrdinalIgnoreCase))
            {
                total += ReplaceAllInCurrentFile(query, replacement);
                continue;
            }
            var text = _catalog.ReadFile(id);
            int count = regex.Matches(text).Count;
            if (count == 0) continue;
            _catalog.WriteFile(id, regex.Replace(text, safeReplacement));
            total += count;
        }

        UpdatePanels();
        return total;
    }
}

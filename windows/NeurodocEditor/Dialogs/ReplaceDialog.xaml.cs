using Microsoft.UI.Xaml.Controls;

namespace NeurodocEditor.Dialogs;

public sealed partial class ReplaceDialog : ContentDialog
{
    private readonly Func<string, string, bool, bool> _replaceOne;
    private readonly Func<string, string, bool, int> _replaceAll;

    /// <param name="replaceOne">(найти, заменитьНа, весьПроект) → заменено ли.</param>
    /// <param name="replaceAll">(найти, заменитьНа, весьПроект) → число замен.</param>
    /// <param name="wholeProjectScope">Начальная область: true — весь проект.</param>
    public ReplaceDialog(
        Func<string, string, bool, bool> replaceOne,
        Func<string, string, bool, int> replaceAll,
        bool wholeProjectScope)
    {
        _replaceOne = replaceOne;
        _replaceAll = replaceAll;
        InitializeComponent();
        ScopeRadios.SelectedIndex = wholeProjectScope ? 1 : 0;
    }

    private bool WholeProject => ScopeRadios.SelectedIndex == 1;

    private bool ValidateQuery(out string query)
    {
        query = FindText.Text;
        if (query.Length == 0)
        {
            ShowStatus(InfoBarSeverity.Warning, "Укажите строку поиска.");
            return false;
        }
        return true;
    }

    private void OnReplaceOneClick(ContentDialog sender, ContentDialogButtonClickEventArgs args)
    {
        args.Cancel = true; // окно остаётся открытым
        if (!ValidateQuery(out var query)) return;
        bool replaced = _replaceOne(query, ReplaceText.Text, WholeProject);
        ShowStatus(
            replaced ? InfoBarSeverity.Success : InfoBarSeverity.Informational,
            replaced ? "Заменено одно вхождение." : "Совпадений не найдено.");
    }

    private void OnReplaceAllClick(ContentDialog sender, ContentDialogButtonClickEventArgs args)
    {
        args.Cancel = true; // окно остаётся открытым
        if (!ValidateQuery(out var query)) return;
        int count = _replaceAll(query, ReplaceText.Text, WholeProject);
        ShowStatus(
            count > 0 ? InfoBarSeverity.Success : InfoBarSeverity.Informational,
            count > 0 ? $"Заменено вхождений: {count}." : "Совпадений не найдено.");
    }

    private void ShowStatus(InfoBarSeverity severity, string message)
    {
        StatusBar.Severity = severity;
        StatusBar.Message = message;
        StatusBar.IsOpen = true;
    }
}

using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace NeurodocEditor.Dialogs;

public sealed partial class HotkeysDialog : ContentDialog
{
    private static readonly (string Keys, string Action)[] Hotkeys =
    {
        ("Ctrl+S  /  F2", "Сохранить файл (автосохранение тоже работает)"),
        ("Ctrl+F", "Поиск по текущему файлу"),
        ("Ctrl+Shift+F", "Поиск по всему проекту"),
        ("Ctrl+R", "Поиск и замена в текущем файле"),
        ("Ctrl+Shift+R", "Поиск и замена по всему проекту"),
        ("Ctrl+G", "Перейти по ссылке, на которой стоит каретка"),
        ("Ctrl+T", "Новая ссылка: вставить, создать файл и перейти"),
        ("Ctrl+U", "Снять ссылку, оставив её текст"),
        ("Ctrl+L", "Связать выделенный текст со ссылкой из буфера обмена"),
        ("Ctrl+N", "Новый файл, не привязанный к другим"),
        ("Ctrl+H", "Перейти к корневому файлу (root)"),
        ("Ctrl+O", "Открыть файл по UUID или пути"),
        ("Ctrl+B", "Битые ссылки в проекте"),
        ("Alt+←  /  Alt+→", "Назад / вперёд по истории (как в браузере)"),
        ("Tab  /  Shift+Tab", "Отступ 4 пробела / снять отступ (и для выделенных строк)"),
        ("Shift+Alt+↑ / ↓", "Переместить текущую строку вверх / вниз"),
        ("Shift+Delete", "Вырезать строку (или выделение) в буфер"),
        ("Ctrl+Y", "Удалить строку без копирования в буфер"),
        ("Ctrl+Z  /  Ctrl+Shift+Z", "Отменить / повторить"),
        ("Enter", "Новая строка с сохранением отступа"),
        ("Клик по ссылке", "Переход к файлу [текст](цель); http(s)-ссылки — в браузере"),
        ("F5", "Перечитать проект с диска"),
        ("F1", "Эта шпаргалка"),
    };

    public HotkeysDialog()
    {
        InitializeComponent();
        foreach (var (keys, action) in Hotkeys)
        {
            var row = new Grid { ColumnSpacing = 14 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(190) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

            var kbd = new Border
            {
                Background = (Brush)Application.Current.Resources["PaneBackgroundBrush"],
                BorderBrush = (Brush)Application.Current.Resources["CardBorderBrush"],
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(4),
                Padding = new Thickness(8, 3, 8, 3),
                HorizontalAlignment = HorizontalAlignment.Left,
                Child = new TextBlock
                {
                    Text = keys,
                    FontFamily = new FontFamily("Consolas"),
                    FontSize = 12,
                },
            };
            Grid.SetColumn(kbd, 0);
            row.Children.Add(kbd);

            var description = new TextBlock
            {
                Text = action,
                FontSize = 13,
                TextWrapping = TextWrapping.Wrap,
                VerticalAlignment = VerticalAlignment.Center,
            };
            Grid.SetColumn(description, 1);
            row.Children.Add(description);

            Rows.Children.Add(row);
        }
    }
}

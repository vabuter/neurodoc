using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace NeurodocEditor.Controls;

/// <summary>
/// Визуальный чип-«пилюля» поверх neurodoc-ссылки: показывает курсор-«руку»
/// (ProtectedCursor доступен только из наследника элемента).
/// </summary>
public sealed partial class ChipView : Grid
{
    // Курсор один на все чипы: их пересоздают при каждой перерисовке оверлея,
    // а это на каждое нажатие клавиши, и заводить системный курсор заново незачем.
    private static readonly InputCursor HandCursor =
        InputSystemCursor.Create(InputSystemCursorShape.Hand);

    public ChipView()
    {
        BorderThickness = new Thickness(1);
        ProtectedCursor = HandCursor;
    }
}

using Microsoft.UI.Input;
using Microsoft.UI.Xaml.Controls;

namespace NeurodocEditor.Controls;

/// <summary>
/// Канва, показывающая курсор-«руку» над своими дочерними элементами (чипами ссылок).
/// Фон канвы прозрачен для хит-теста, поэтому курсор меняется только над чипами.
/// </summary>
public sealed partial class HandCanvas : Canvas
{
    public HandCanvas()
    {
        ProtectedCursor = InputSystemCursor.Create(InputSystemCursorShape.Hand);
    }
}

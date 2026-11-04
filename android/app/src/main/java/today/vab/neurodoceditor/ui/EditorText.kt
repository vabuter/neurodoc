package today.vab.neurodoceditor.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.style.TextDecoration
import today.vab.neurodoceditor.data.NeurodocCatalog

/** Ссылка в тексте: neurodoc-ссылка на файл или веб-адрес. */
data class TextLink(
    val range: IntRange,
    /** Файл каталога, куда ведёт ссылка, или сам веб-адрес. */
    val target: String,
    val isWeb: Boolean,
    /** Текст ссылки из `[текст](цель)`; у веб-адресов пуст. */
    val anchor: String = "",
)

/** Значок, которым показывается ссылка без якоря. */
const val LINK_BULLET = "●"

/** Чем ссылка показывается: своим текстом, а без него — значком. */
fun linkLabel(link: TextLink): String = link.anchor.ifEmpty { LINK_BULLET }

/**
 * Ссылки, которые схлопываются до якоря. В режиме «как есть» не схлопывается ничто,
 * а веб-адрес виден целиком всегда — прятать в нём нечего.
 */
fun collapsedLinks(links: List<TextLink>, rawEditing: Boolean): List<TextLink> =
    if (rawEditing) emptyList() else links.filter { !it.isWeb }

/**
 * Находит в тексте neurodoc-ссылки и http(s)-адреса.
 *
 * Цель ссылки считается ОТ ФАЙЛА, в котором она написана, поэтому нужен [sourceId]:
 * `[auth.start](../contract/auth/start.yaml)` из файла в _content ведёт на
 * contract/auth/start.yaml. Неразобравшаяся цель ведёт за пределы каталога — такая
 * ссылка не рисуется и попадает в «Битые ссылки».
 */
fun findLinks(text: String, sourceId: String): List<TextLink> {
    val links = mutableListOf<TextLink>()
    NeurodocCatalog.LINK_REGEX.findAll(text).forEach { m ->
        val raw = m.groupValues[2]
        val anchor = m.groupValues[1]
        if (NeurodocCatalog.isWebTarget(raw)) {
            links.add(TextLink(m.range, raw, isWeb = true, anchor = anchor))
        } else {
            val id = NeurodocCatalog.tryResolveLink(sourceId, raw) ?: return@forEach
            links.add(TextLink(m.range, id, isWeb = false, anchor = anchor))
        }
    }
    NeurodocCatalog.WEB_LINK_REGEX.findAll(text).forEach { m ->
        // адрес внутри [текста](…) уже учтён выше — второй раз подсвечивать незачем
        if (links.any { m.range.first >= it.range.first && m.range.first <= it.range.last }) return@forEach
        // завершающая пунктуация предложения — не часть адреса
        val url = m.value.trimEnd('.', ',', ';', ':', '!', '?', ')', ']', '\'', '"')
        if (url.contains("://") && url.substringAfter("://").isNotEmpty()) {
            links.add(TextLink(m.range.first until m.range.first + url.length, url, isWeb = true))
        }
    }
    return links.sortedBy { it.range.first }
}

/**
 * Готовит текст для показа: подсвечивает ссылки и совпадения поиска, а обычном режиме
 * прячет служебную часть ссылки — от `[текст](цель)` остаётся только текст.
 * Сам файл при этом не меняется.
 *
 * @param isRead прочитан ли файл-цель: пройденные ссылки гасим, как ":visited"
 * в браузере — по виду ссылки читатель должен помнить, был он там или нет.
 */
fun transformEditorText(
    text: String,
    links: List<TextLink>,
    rawEditing: Boolean,
    linkColor: Color,
    webLinkColor: Color,
    visitedColor: Color,
    isRead: (String) -> Boolean,
    findMatches: List<IntRange>,
    findCurrent: Int,
    findHighlight: Color,
    findCurrentColor: Color,
): TransformedText {
    fun colorOf(link: TextLink): Color = when {
        link.isWeb -> webLinkColor
        isRead(link.target) -> visitedColor
        else -> linkColor
    }

    // ссылки-файлы, которые схлопываем до текста якоря (или до значка, если якоря нет)
    val collapsed = collapsedLinks(links, rawEditing)
    val labels = collapsed.map { linkLabel(it) }

    val builder = AnnotatedString.Builder()
    val mapping = CollapseOffsetMapping(collapsed.map { it.range }, labels.map { it.length })

    var cursor = 0
    collapsed.forEachIndexed { index, link ->
        val start = link.range.first.coerceIn(0, text.length)
        val end = (link.range.last + 1).coerceIn(0, text.length)
        if (start < cursor || start >= end) return@forEachIndexed
        builder.append(text, cursor, start)
        val labelStart = builder.length
        builder.append(labels[index])
        builder.addStyle(
            SpanStyle(
                color = colorOf(link),
                fontWeight = FontWeight.Medium,
                // подчёркивание, а не заливка: при переносе строки якорь разрывается,
                // и фон смотрелся бы рвано (та же причина, что в Windows-версии)
                textDecoration = if (link.anchor.isEmpty()) null else TextDecoration.Underline,
            ),
            labelStart, builder.length,
        )
        cursor = end
    }
    if (cursor < text.length) builder.append(text, cursor, text.length)

    // стили для тех ссылок, что остались в тексте как есть (веб-адреса и режим «как есть»)
    for (link in links) {
        if (link in collapsed) continue
        val start = mapping.originalToTransformed(link.range.first.coerceIn(0, text.length))
        val end = mapping.originalToTransformed((link.range.last + 1).coerceIn(0, text.length))
        if (start >= end) continue
        builder.addStyle(
            SpanStyle(
                color = colorOf(link),
                fontWeight = FontWeight.Medium,
                textDecoration = TextDecoration.Underline,
            ),
            start, end,
        )
    }

    findMatches.forEachIndexed { index, range ->
        val start = mapping.originalToTransformed(range.first.coerceIn(0, text.length))
        val end = mapping.originalToTransformed((range.last + 1).coerceIn(0, text.length))
        if (start >= end) return@forEachIndexed
        builder.addStyle(
            SpanStyle(background = if (index == findCurrent) findCurrentColor else findHighlight),
            start, end,
        )
    }

    return TransformedText(builder.toAnnotatedString(), mapping)
}

/**
 * Прямоугольники диапазона показанного текста — по одному на строку.
 *
 * Нужны, чтобы тап засчитывался только по самим буквам ссылки. Позиция символа тут не
 * годится: getOffsetForPosition возвращает БЛИЖАЙШИЙ символ, даже если палец далеко за
 * концом строки, — от этого пустое место справа от ссылки открывало файл, будто ссылка
 * растянута на всю ширину экрана. Перенесённый на другую строку якорь тоже надо
 * разбирать построчно, иначе общая рамка накроет и чужой текст.
 */
fun lineRects(
    layout: androidx.compose.ui.text.TextLayoutResult,
    from: Int,
    to: Int,
): List<androidx.compose.ui.geometry.Rect> {
    if (to <= from) return emptyList()
    val lastIndex = (to - 1).coerceAtLeast(from)
    val firstLine = layout.getLineForOffset(from)
    val lastLine = layout.getLineForOffset(lastIndex)
    return (firstLine..lastLine).mapNotNull { line ->
        val left =
            if (line == firstLine) layout.getHorizontalPosition(from, usePrimaryDirection = true)
            else layout.getLineLeft(line)
        val right =
            if (line == lastLine) layout.getHorizontalPosition(to, usePrimaryDirection = true)
            else layout.getLineRight(line)
        if (right <= left) null
        else androidx.compose.ui.geometry.Rect(
            left, layout.getLineTop(line), right, layout.getLineBottom(line),
        )
    }
}

/**
 * Сопоставление позиций, когда куски исходного текста схлопываются в короткий значок.
 * Нужно, чтобы каретка, выделение и тапы попадали в правильные места.
 */
class CollapseOffsetMapping(
    private val ranges: List<IntRange>,
    /** Длина замены для каждого диапазона: у якорей она своя, а не общая. */
    private val replacementLengths: List<Int>,
) : OffsetMapping {

    private fun replacementAt(index: Int) = replacementLengths.getOrElse(index) { 0 }

    override fun originalToTransformed(offset: Int): Int {
        var shift = 0
        for (i in ranges.indices) {
            val range = ranges[i]
            val start = range.first
            val end = range.last + 1
            val replacement = replacementAt(i)
            when {
                offset <= start -> return offset - shift
                offset < end -> return start - shift + replacement
                else -> shift += (end - start) - replacement
            }
        }
        return offset - shift
    }

    override fun transformedToOriginal(offset: Int): Int {
        var shift = 0
        for (i in ranges.indices) {
            val range = ranges[i]
            val replacement = replacementAt(i)
            val start = range.first - shift
            val end = start + replacement
            when {
                offset <= start -> return offset + shift
                offset < end -> return range.first
                else -> shift += (range.last + 1 - range.first) - replacement
            }
        }
        return offset + shift
    }
}

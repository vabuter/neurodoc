package today.vab.neurodoceditor.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Create
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import today.vab.neurodoceditor.data.AppSettings
import today.vab.neurodoceditor.data.FileRef
import today.vab.neurodoceditor.data.NeurodocCatalog
import today.vab.neurodoceditor.ui.theme.LocalNeurodocColors

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditorScreen(
    vm: EditorViewModel,
    onOpenProjects: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenHelp: () -> Unit,
    onOpenReplace: () -> Unit,
    onOpenById: () -> Unit,
    onDeleteRequest: () -> Unit,
    onCopyLink: () -> Unit,
    onCopyContent: () -> Unit,
    onWebLink: (String) -> Unit,
    onLinkSelection: () -> Unit,
    onOpenGit: () -> Unit,
) {
    val colors = LocalNeurodocColors.current
    var menuOpen by remember { mutableStateOf(false) }
    var findBarVisible by remember { mutableStateOf(false) }
    var panelsVisible by remember { mutableStateOf(false) }
    var projectSearchVisible by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            Column {
                TopAppBar(
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.background,
                    ),
                    title = {
                        Column {
                            Text(
                                text = (vm.settings.currentProject?.name ?: "Neurodoc Editor") +
                                    (vm.gitBranch?.let { " · $it" } ?: ""),
                                fontSize = 16.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            vm.currentId?.let { id ->
                                Text(
                                    text = if (id == NeurodocCatalog.ROOT_ID) "root" else id,
                                    fontSize = 10.sp,
                                    fontFamily = FontFamily.Monospace,
                                    color = colors.dimText,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    },
                    navigationIcon = {
                        IconButton(onClick = { vm.goBack() }, enabled = vm.canGoBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Назад")
                        }
                    },
                    actions = {
                        IconButton(onClick = { vm.goForward() }, enabled = vm.canGoForward) {
                            Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = "Вперёд")
                        }
                        IconButton(onClick = { vm.goHome() }, enabled = vm.catalog != null) {
                            Icon(Icons.Default.Home, contentDescription = "К корневому файлу")
                        }
                        IconButton(onClick = { vm.insertNewLink() }, enabled = vm.catalog != null) {
                            Icon(Icons.Default.Add, contentDescription = "Новая ссылка")
                        }
                        IconButton(onClick = { menuOpen = true }) {
                            Icon(Icons.Default.MoreVert, contentDescription = "Меню")
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            DropdownMenuItem(
                                text = { Text("Новый файл") },
                                leadingIcon = { Icon(Icons.Default.Create, null) },
                                onClick = { menuOpen = false; vm.createUnlinkedFile() },
                            )
                            DropdownMenuItem(
                                text = { Text("Открыть по UUID или пути") },
                                leadingIcon = { Icon(Icons.Default.Info, null) },
                                onClick = { menuOpen = false; onOpenById() },
                            )
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text("Связать с ссылкой из буфера") },
                                enabled = vm.currentId != null,
                                onClick = { menuOpen = false; onLinkSelection() },
                            )
                            DropdownMenuItem(
                                text = { Text("Снять ссылку под курсором") },
                                enabled = vm.currentId != null,
                                onClick = {
                                    menuOpen = false
                                    if (!vm.removeLinkAtCaret()) {
                                        vm.statusMessage = "Курсор не на ссылке"
                                    }
                                },
                            )
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text("Поиск в файле") },
                                leadingIcon = { Icon(Icons.Default.Search, null) },
                                onClick = { menuOpen = false; findBarVisible = true },
                            )
                            DropdownMenuItem(
                                text = { Text("Поиск по проекту") },
                                leadingIcon = { Icon(Icons.Default.Search, null) },
                                onClick = { menuOpen = false; projectSearchVisible = true },
                            )
                            DropdownMenuItem(
                                text = { Text("Поиск и замена") },
                                onClick = { menuOpen = false; onOpenReplace() },
                            )
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text("Копировать ссылку") },
                                leadingIcon = { Icon(Icons.Default.Share, null) },
                                enabled = vm.currentId != null && !vm.isRoot,
                                onClick = { menuOpen = false; onCopyLink() },
                            )
                            DropdownMenuItem(
                                text = { Text("Копировать содержимое") },
                                onClick = { menuOpen = false; onCopyContent() },
                            )
                            DropdownMenuItem(
                                text = { Text("Связи файла") },
                                onClick = { menuOpen = false; panelsVisible = true },
                            )
                            DropdownMenuItem(
                                text = { Text("Обновить с диска") },
                                onClick = { menuOpen = false; vm.reloadFromDisk() },
                            )
                            DropdownMenuItem(
                                text = { Text("Git…") },
                                enabled = vm.catalog != null,
                                onClick = { menuOpen = false; onOpenGit() },
                            )
                            DropdownMenuItem(
                                text = { Text("Сбросить прогресс чтения") },
                                enabled = vm.catalog != null,
                                onClick = { menuOpen = false; vm.resetReadingProgress() },
                            )
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text("Удалить файл") },
                                leadingIcon = { Icon(Icons.Default.Delete, null) },
                                enabled = vm.currentId != null && !vm.isRoot,
                                onClick = { menuOpen = false; onDeleteRequest() },
                            )
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text("Проекты") },
                                onClick = { menuOpen = false; onOpenProjects() },
                            )
                            DropdownMenuItem(
                                text = { Text("Настройки") },
                                onClick = { menuOpen = false; onOpenSettings() },
                            )
                            DropdownMenuItem(
                                text = { Text("Справка") },
                                onClick = { menuOpen = false; onOpenHelp() },
                            )
                        }
                    },
                )
                Breadcrumbs(vm)
                ReadingProgressLine(vm)
                ChipBar(vm)
                if (vm.gitBusy) {
                    androidx.compose.material3.LinearProgressIndicator(modifier = Modifier.fillMaxWidth().height(3.dp))
                }
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding(),
        ) {
            if (findBarVisible) {
                FindBar(
                    vm = vm,
                    onClose = { findBarVisible = false; vm.closeFind() },
                )
            }
            if (projectSearchVisible) {
                ProjectSearchBar(
                    vm = vm,
                    onClose = { projectSearchVisible = false; vm.clearProjectSearch() },
                    onHitClick = { vm.openSearchHit(it) },
                )
            }
            Box(modifier = Modifier.weight(1f)) {
                when {
                    vm.catalog == null -> EmptyState(onOpenProjects)
                    vm.gitDiff != null -> GitDiffViewer(vm)
                    vm.viewMode != ViewMode.TEXT -> FormatViewer(vm, onWebLink)
                    else -> EditorArea(vm, onWebLink)
                }
            }
            if (vm.gitHistoryVisible) {
                GitHistoryPanel(vm)
            }
            if (panelsVisible) {
                LinkPanels(
                    vm = vm,
                    onClose = { panelsVisible = false },
                    onOpen = { id -> vm.openFile(id); panelsVisible = false },
                )
            }
        }
    }
}

@Composable
private fun EmptyState(onOpenProjects: () -> Unit) {
    val colors = LocalNeurodocColors.current
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("Проект не выбран", fontSize = 18.sp)
        Spacer(Modifier.height(8.dp))
        Text(
            "Добавьте проект — название и путь к каталогу neurodoc.",
            fontSize = 13.sp,
            color = colors.dimText,
        )
        Spacer(Modifier.height(20.dp))
        androidx.compose.material3.Button(onClick = onOpenProjects) {
            Text("Открыть список проектов")
        }
    }
}

/** Хлебные крошки: кратчайшие пути от root, с горизонтальной прокруткой. */
@Composable
private fun Breadcrumbs(vm: EditorViewModel) {
    val colors = LocalNeurodocColors.current
    if (vm.breadcrumbs.isEmpty()) return
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp)
            .padding(bottom = 4.dp),
    ) {
        vm.breadcrumbs.take(3).forEach { path ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState()),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                path.forEachIndexed { index, ref ->
                    if (index > 0) {
                        Text(" › ", fontSize = 12.sp, color = colors.dimText)
                    }
                    val isLast = index == path.lastIndex
                    Text(
                        text = ref.title,
                        fontSize = 12.sp,
                        maxLines = 1,
                        color = if (isLast) colors.dimText else MaterialTheme.colorScheme.primary,
                        modifier = if (isLast) Modifier
                        else Modifier.clickable { vm.openFile(ref.id) },
                    )
                }
            }
        }
    }
}

/**
 * Прогресс чтения тонкой полосой: на телефоне место дорого, а «сколько пройдено»
 * должно быть видно всегда. Счётчик и кнопки — в строке чипов ниже.
 */
@Composable
private fun ReadingProgressLine(vm: EditorViewModel) {
    if (vm.catalog == null || vm.readTotal == 0) return
    androidx.compose.material3.LinearProgressIndicator(
        progress = { vm.readCount.toFloat() / vm.readTotal },
        modifier = Modifier.fillMaxWidth().height(3.dp),
        trackColor = LocalNeurodocColors.current.cardBorder,
        drawStopIndicator = {},
    )
}

/** Один чип строки под шапкой. */
@Composable
private fun Chip(label: String, selected: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    val colors = LocalNeurodocColors.current
    Surface(
        shape = RoundedCornerShape(6.dp),
        color = if (selected) MaterialTheme.colorScheme.primaryContainer else colors.paneBackground,
        border = androidx.compose.foundation.BorderStroke(1.dp, colors.cardBorder),
        modifier = if (enabled) Modifier.clickable { onClick() } else Modifier,
    ) {
        Text(
            text = label,
            fontSize = 12.sp,
            color = when {
                !enabled -> colors.dimText
                selected -> MaterialTheme.colorScheme.onPrimaryContainer
                else -> MaterialTheme.colorScheme.onSurface
            },
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp),
        )
    }
}

/**
 * Строка чипов: чтение, режим «как есть» и форматы просмотра.
 *
 * Всё в одной прокручиваемой строке намеренно — на телефоне каждая лишняя полоса
 * в шапке отъедает текст, ради которого приложение и открывают.
 */
@Composable
private fun ChipBar(vm: EditorViewModel) {
    if (vm.catalog == null) return
    val colors = LocalNeurodocColors.current
    // MD сразу после «Как есть»: из отрисовываемых форматов он нужен чаще прочих
    val modes = listOf(
        ViewMode.MD to "MD",
        ViewMode.SVG to "SVG",
        ViewMode.JSON to "JSON",
        ViewMode.YAML to "YAML",
        ViewMode.BASH to "Bash",
        ViewMode.ADOC to "AsciiDoc",
        ViewMode.HTML to "HTML",
        ViewMode.NGINX to "nginx",
    )
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 12.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (vm.readTotal > 0) {
            Text(
                text = "${vm.readCount}/${vm.readTotal}",
                fontSize = 11.sp,
                color = colors.dimText,
                maxLines = 1,
            )
        }
        // отметку о прочтении ставят только neurodoc-файлам: контракты читают
        // по необходимости, а не подряд, и в обход чтения они не входят
        Chip(
            label = "Прочитано",
            selected = vm.isCurrentRead,
            enabled = vm.isNeurodocFile,
        ) { vm.toggleCurrentRead() }
        Chip(label = "Далее", selected = false, enabled = vm.hasNextUnread) { vm.openNextUnread() }
        Text("│", fontSize = 12.sp, color = colors.cardBorder)
        // «как есть»: файл ровно как на диске, служебные части ссылок не прячутся
        Chip(label = "Как есть", selected = vm.settings.rawEditing) {
            vm.setRawEditing(!vm.settings.rawEditing)
        }
        Text("│", fontSize = 12.sp, color = colors.cardBorder)
        modes.forEach { (mode, label) ->
            val selected = vm.viewMode == mode
            Chip(label = label, selected = selected) {
                vm.changeViewMode(if (selected) ViewMode.TEXT else mode)
            }
        }
    }
}

/** Область редактирования с подсветкой ссылок и переходом по тапу. */
@Composable
private fun EditorArea(vm: EditorViewModel, onWebLink: (String) -> Unit) {
    val colors = LocalNeurodocColors.current
    val settings = vm.settings
    // цель ссылки считается ОТ ФАЙЛА, в котором она написана: ссылка по пути
    // без источника неполна. Разбираем текст редактора, а не сохранённую копию —
    // автосохранение приходит с задержкой, а подсветка нужна сразу.
    val sourceId = vm.currentId ?: NeurodocCatalog.ROOT_ID
    val links = remember(vm.text.text, sourceId) { findLinks(vm.text.text, sourceId) }
    val offsetMapping = remember(links, settings.rawEditing) {
        val collapsed = collapsedLinks(links, settings.rawEditing)
        CollapseOffsetMapping(collapsed.map { it.range }, collapsed.map { linkLabel(it).length })
    }
    var layoutResult by remember { mutableStateOf<androidx.compose.ui.text.TextLayoutResult?>(null) }
    val focusRequester = remember { FocusRequester() }
    val scrollState = rememberScrollState()

    // Возврат «назад» должен возвращать и прокрутку, а не только каретку: иначе
    // экран прыгает в начало файла.
    androidx.compose.runtime.LaunchedEffect(vm.currentId) {
        val target = vm.pendingScroll
        if (target > 0) {
            // раскладка ещё не досчитана: maxValue равен нулю, и scrollTo увёл бы
            // в начало файла вместо запомненного места
            var waited = 0
            while (scrollState.maxValue == 0 && waited < 500) {
                kotlinx.coroutines.delay(16)
                waited += 16
            }
            runCatching { scrollState.scrollTo(target.coerceAtMost(scrollState.maxValue)) }
        }
        vm.scrollRestored()
    }
    // именно snapshotFlow: LaunchedEffect(scrollState.value) заводил бы корутину
    // на каждый пиксель прокрутки
    androidx.compose.runtime.LaunchedEffect(scrollState) {
        androidx.compose.runtime.snapshotFlow { scrollState.value }
            .collect { vm.onScrollChanged(it) }
    }

    val fontFamily = when (settings.fontFamily) {
        AppSettings.FONT_JETBRAINS -> today.vab.neurodoceditor.ui.theme.JetBrainsMonoFamily
        AppSettings.FONT_INTER -> today.vab.neurodoceditor.ui.theme.InterFamily
        else -> FontFamily.Default
    }
    // цвета берём заранее: внутри visualTransformation @Composable-контекста уже нет
    val linkColor = MaterialTheme.colorScheme.primary
    val visitedColor = colors.visitedLink
    // пройденные ссылки гасим, как ":visited" в браузере
    val readIds = remember(vm.readCount, links) { links.filter { vm.isRead(it.target) }.map { it.target }.toSet() }
    // В режиме «как есть» тап по ссылке не перехватываем совсем: он и нужен затем,
    // чтобы поставить курсор внутрь ссылки и поправить её руками. Так же ведёт себя
    // Windows-версия — там в этом режиме оверлей со ссылками просто не рисуется.
    val tappableLinks = if (settings.rawEditing) emptyList() else links
    val onSurfaceColor = MaterialTheme.colorScheme.onSurface
    val textStyle = TextStyle(
        color = onSurfaceColor,
        fontSize = settings.fontSize.sp,
        lineHeight = (settings.fontSize * settings.lineSpacing).sp,
        fontFamily = fontFamily,
    )

    Surface(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 8.dp, vertical = 4.dp),
        color = colors.editorBackground,
        shape = RoundedCornerShape(10.dp),
        border = androidx.compose.foundation.BorderStroke(1.dp, colors.cardBorder),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(scrollState)
                .padding(14.dp),
        ) {
            BasicTextField(
                value = vm.text,
                onValueChange = { vm.onTextChange(it) },
                textStyle = textStyle,
                cursorBrush = androidx.compose.ui.graphics.SolidColor(linkColor),
                onTextLayout = { layoutResult = it },
                visualTransformation = { annotated ->
                    transformEditorText(
                        text = annotated.text,
                        links = links,
                        rawEditing = settings.rawEditing,
                        linkColor = linkColor,
                        webLinkColor = colors.webLink,
                        visitedColor = visitedColor,
                        isRead = { it in readIds },
                        findMatches = vm.findMatches,
                        findCurrent = vm.findCurrent,
                        findHighlight = colors.findHighlight,
                        findCurrentColor = colors.findCurrent,
                    )
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(focusRequester)
                    // тап по ссылке перехватываем до текстового поля
                    .pointerInput(tappableLinks, layoutResult, offsetMapping) {
                        awaitPointerEventScope {
                            while (true) {
                                val event = awaitPointerEvent(PointerEventPass.Initial)
                                val change = event.changes.firstOrNull() ?: continue
                                if (!change.pressed) continue
                                if (tappableLinks.isEmpty()) continue
                                val layout = layoutResult ?: continue
                                // Ищем ссылку по её прямоугольникам, а не по позиции символа:
                                // getOffsetForPosition отдаёт ближайший символ даже для тапа
                                // далеко за строкой, и пустое место справа от ссылки
                                // открывало файл, будто ссылка занимает всю строку.
                                val link = tappableLinks.firstOrNull { candidate ->
                                    val from = offsetMapping.originalToTransformed(candidate.range.first)
                                    val to = offsetMapping.originalToTransformed(candidate.range.last + 1)
                                    lineRects(layout, from, to).any { it.contains(change.position) }
                                }
                                if (link != null) {
                                    change.consume()
                                    // ждём отпускания, чтобы не мешать прокрутке
                                    var released = false
                                    while (!released) {
                                        val next = awaitPointerEvent(PointerEventPass.Initial)
                                        val c = next.changes.firstOrNull() ?: break
                                        if (!c.pressed) {
                                            released = true
                                            c.consume()
                                            if (link.isWeb) onWebLink(link.target)
                                            else vm.openFile(link.target)
                                        } else {
                                            c.consume()
                                        }
                                    }
                                }
                            }
                        }
                    },
            )
        }
    }
}

/** Панель поиска по текущему файлу. */
@Composable
private fun FindBar(vm: EditorViewModel, onClose: () -> Unit) {
    val colors = LocalNeurodocColors.current
    val focusRequester = remember { FocusRequester() }
    androidx.compose.runtime.LaunchedEffect(Unit) { runCatching { focusRequester.requestFocus() } }

    Surface(color = colors.paneBackground) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextField(
                value = vm.findQuery,
                onValueChange = { vm.updateFindQuery(it) },
                placeholder = { Text("Найти в файле", fontSize = 14.sp) },
                singleLine = true,
                modifier = Modifier.weight(1f).focusRequester(focusRequester),
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                    unfocusedContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                ),
            )
            Text(
                text = if (vm.findMatches.isEmpty()) "0"
                else "${vm.findCurrent + 1} из ${vm.findMatches.size}",
                fontSize = 12.sp,
                color = colors.dimText,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            IconButton(onClick = { vm.findStep(false) }) {
                Icon(Icons.Default.KeyboardArrowUp, contentDescription = "Предыдущее")
            }
            IconButton(onClick = { vm.findStep(true) }) {
                Icon(Icons.Default.KeyboardArrowDown, contentDescription = "Следующее")
            }
            IconButton(onClick = onClose) {
                Icon(Icons.Default.Close, contentDescription = "Закрыть")
            }
        }
    }
}

/** Панель поиска по всему проекту с результатами. */
@Composable
private fun ProjectSearchBar(
    vm: EditorViewModel,
    onClose: () -> Unit,
    onHitClick: (today.vab.neurodoceditor.data.SearchHit) -> Unit,
) {
    val colors = LocalNeurodocColors.current
    var query by remember { mutableStateOf(vm.searchQuery) }
    val focusManager = LocalFocusManager.current

    Surface(color = colors.paneBackground) {
        Column(modifier = Modifier.fillMaxWidth()) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextField(
                    value = query,
                    onValueChange = { query = it },
                    placeholder = { Text("Поиск по проекту", fontSize = 14.sp) },
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                    colors = TextFieldDefaults.colors(
                        focusedContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                        unfocusedContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        imeAction = androidx.compose.ui.text.input.ImeAction.Search,
                    ),
                    keyboardActions = androidx.compose.foundation.text.KeyboardActions(
                        onSearch = {
                            vm.runProjectSearch(query)
                            focusManager.clearFocus()
                        },
                    ),
                )
                IconButton(onClick = { vm.runProjectSearch(query) }) {
                    Icon(Icons.Default.Search, contentDescription = "Искать")
                }
                IconButton(onClick = onClose) {
                    Icon(Icons.Default.Close, contentDescription = "Закрыть")
                }
            }
            if (vm.searchResults.isNotEmpty()) {
                Text(
                    text = "Найдено: ${vm.searchResults.size}",
                    fontSize = 12.sp,
                    color = colors.dimText,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 2.dp),
                )
                androidx.compose.foundation.lazy.LazyColumn(
                    modifier = Modifier.fillMaxWidth().height(200.dp),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 4.dp),
                ) {
                    items(vm.searchResults.size) { index ->
                        val hit = vm.searchResults[index]
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { onHitClick(hit) }
                                .padding(vertical = 6.dp),
                        ) {
                            Text(hit.title, fontSize = 11.sp, color = colors.dimText, maxLines = 1)
                            Text(
                                text = hit.prefix + hit.match + hit.suffix,
                                fontSize = 13.sp,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
        }
    }
}

/** Панели связей: обратные ссылки и файлы без входящих ссылок. */
@Composable
private fun LinkPanels(vm: EditorViewModel, onClose: () -> Unit, onOpen: (String) -> Unit) {
    val colors = LocalNeurodocColors.current
    Surface(
        color = colors.paneBackground,
        modifier = Modifier.fillMaxWidth().height(280.dp),
    ) {
        Column(modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Связи файла", fontSize = 14.sp, modifier = Modifier.weight(1f))
                IconButton(onClick = onClose) {
                    Icon(Icons.Default.Close, contentDescription = "Закрыть")
                }
            }
            androidx.compose.foundation.lazy.LazyColumn(modifier = Modifier.fillMaxWidth()) {
                item { PanelHeader("Ссылаются на этот файл", first = true) }
                if (vm.backlinks.isEmpty()) {
                    item { PanelEmpty("Нет входящих ссылок") }
                } else {
                    items(vm.backlinks.size) { index ->
                        FileRefRow(vm.backlinks[index]) { onOpen(it) }
                    }
                }
                if (vm.brokenLinks.isNotEmpty()) {
                    item {
                        PanelHeader(
                            "Битые ссылки: " + vm.brokenLinks.size,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                    items(vm.brokenLinks.size) { index ->
                        val link = vm.brokenLinks[index]
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { vm.openBrokenLink(link); onClose() }
                                .padding(vertical = 6.dp),
                        ) {
                            Text(link.fileTitle, fontSize = 13.sp, maxLines = 1,
                                overflow = TextOverflow.Ellipsis)
                            Text(link.context, fontSize = 11.sp, color = colors.dimText,
                                maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(link.targetId, fontSize = 10.sp, fontFamily = FontFamily.Monospace,
                                color = MaterialTheme.colorScheme.error, maxLines = 1)
                        }
                    }
                }
                item { PanelHeader("Файлы без входящих ссылок") }
                if (vm.orphans.isEmpty()) {
                    item { PanelEmpty("Нет таких файлов") }
                } else {
                    items(vm.orphans.size) { index ->
                        FileRefRow(vm.orphans[index]) { onOpen(it) }
                    }
                }
            }
        }
    }
}

/**
 * Заголовок раздела в панели связей.
 *
 * Раньше заголовок и строка «Нет входящих ссылок» рисовались одинаково — тем же
 * размером и тем же приглушённым цветом, и раздел было не отличить от его содержимого.
 * Теперь заголовок жирный, полного контраста и отделён чертой.
 */
@Composable
private fun PanelHeader(
    text: String,
    first: Boolean = false,
    color: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.onSurface,
) {
    val colors = LocalNeurodocColors.current
    Column(modifier = Modifier.fillMaxWidth()) {
        if (!first) {
            HorizontalDivider(
                color = colors.cardBorder,
                modifier = Modifier.padding(top = 12.dp, bottom = 8.dp),
            )
        }
        Text(
            text = text,
            fontSize = 13.sp,
            fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
            color = color,
            modifier = Modifier.padding(bottom = 4.dp),
        )
    }
}

/** Строка «в этом разделе пусто» — намеренно негромкая, в отличие от заголовка. */
@Composable
private fun PanelEmpty(text: String) {
    Text(
        text = text,
        fontSize = 12.sp,
        color = LocalNeurodocColors.current.dimText,
        fontStyle = androidx.compose.ui.text.font.FontStyle.Italic,
        modifier = Modifier.padding(vertical = 2.dp),
    )
}

@Composable
private fun FileRefRow(ref: FileRef, onClick: (String) -> Unit) {
    val colors = LocalNeurodocColors.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onClick(ref.id) }
            .padding(vertical = 6.dp),
    ) {
        Text(ref.title, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(
            ref.id,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = colors.dimText,
            maxLines = 1,
        )
    }
}

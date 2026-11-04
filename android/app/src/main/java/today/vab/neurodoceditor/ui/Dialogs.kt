package today.vab.neurodoceditor.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import today.vab.neurodoceditor.BuildConfig
import today.vab.neurodoceditor.data.AppSettings
import today.vab.neurodoceditor.data.LegacyScan
import today.vab.neurodoceditor.data.NeurodocCatalog
import today.vab.neurodoceditor.data.ProjectInfo
import today.vab.neurodoceditor.data.ThemeMode
import today.vab.neurodoceditor.ui.theme.LocalNeurodocColors

/** Список проектов: добавить / изменить / удалить / открыть. */
@Composable
fun ProjectsDialog(
    vm: EditorViewModel,
    onPickFolder: (ProjectInfo) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = LocalNeurodocColors.current
    var editing by remember { mutableStateOf<ProjectInfo?>(null) }

    if (editing != null) {
        ProjectEditDialog(
            project = editing!!,
            onPickFolder = { onPickFolder(editing!!) },
            onSave = { project ->
                vm.addOrUpdateProject(project)
                editing = null
            },
            onDismiss = { editing = null },
        )
        return
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Проекты") },
        text = {
            Column {
                Text(
                    "Проект — это название и путь к каталогу neurodoc.",
                    fontSize = 12.sp,
                    color = colors.dimText,
                )
                if (vm.settings.projects.isEmpty()) {
                    Text(
                        "Список пуст. Добавьте первый проект.",
                        fontSize = 13.sp,
                        color = colors.dimText,
                        modifier = Modifier.padding(top = 12.dp),
                    )
                } else {
                    LazyColumn(modifier = Modifier.heightIn(max = 320.dp).padding(top = 8.dp)) {
                        items(vm.settings.projects.size) { index ->
                            val project = vm.settings.projects[index]
                            val isCurrent = project.id == vm.settings.currentProjectId
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable {
                                        vm.openProject(project)
                                        onDismiss()
                                    }
                                    .padding(vertical = 8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        text = project.name + if (isCurrent) "  •" else "",
                                        fontSize = 15.sp,
                                        color = if (isCurrent) MaterialTheme.colorScheme.primary
                                        else MaterialTheme.colorScheme.onSurface,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    Text(
                                        text = project.path,
                                        fontSize = 11.sp,
                                        color = colors.dimText,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                                IconButton(onClick = { editing = project }) {
                                    Icon(Icons.Default.Edit, contentDescription = "Изменить")
                                }
                                IconButton(onClick = { vm.deleteProject(project) }) {
                                    Icon(Icons.Default.Delete, contentDescription = "Удалить")
                                }
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { editing = ProjectInfo(name = "", path = "") }) {
                Text("Добавить")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Закрыть") } },
    )
}

@Composable
private fun ProjectEditDialog(
    project: ProjectInfo,
    onPickFolder: () -> Unit,
    onSave: (ProjectInfo) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = LocalNeurodocColors.current
    var name by remember { mutableStateOf(project.name) }
    var path by remember { mutableStateOf(project.path) }
    var error by remember { mutableStateOf<String?>(null) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (project.name.isEmpty()) "Новый проект" else "Изменить проект") },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Название") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = path,
                    onValueChange = { path = it },
                    label = { Text("Путь к каталогу") },
                    placeholder = { Text("/storage/emulated/0/Develop/neurodoc_example") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                TextButton(onClick = onPickFolder, modifier = Modifier.padding(top = 4.dp)) {
                    Text("Выбрать папку…")
                }
                Text(
                    "Если каталога нет, он будет создан вместе с root, content и ReadMe.md.",
                    fontSize = 11.sp,
                    color = colors.dimText,
                )
                error?.let {
                    Text(it, fontSize = 12.sp, color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(top = 6.dp))
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                when {
                    name.isBlank() -> error = "Укажите название проекта."
                    path.isBlank() -> error = "Укажите путь к каталогу."
                    else -> {
                        val result = runCatching { NeurodocCatalog.ensureStructure(path.trim()) }
                        if (result.isSuccess) {
                            onSave(project.copy(name = name.trim(), path = path.trim()))
                        } else {
                            error = "Нет доступа к каталогу: ${result.exceptionOrNull()?.message}"
                        }
                    }
                }
            }) { Text("Сохранить") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Настройки: тема, шрифт, размер, междустрочный интервал. */
@Composable
fun SettingsDialog(vm: EditorViewModel, onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    val settings = vm.settings

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Настройки") },
        text = {
            Column {
                Text("Тема оформления", fontSize = 13.sp, color = colors.dimText)
                val themes = listOf(
                    ThemeMode.SYSTEM to "Системная",
                    ThemeMode.LIGHT to "Светлая",
                    ThemeMode.DARK to "Тёмная",
                )
                themes.forEach { (mode, label) ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .selectable(
                                selected = settings.theme == mode,
                                onClick = { vm.updateSettings(settings.copy(theme = mode)) },
                            )
                            .padding(vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = settings.theme == mode,
                            onClick = { vm.updateSettings(settings.copy(theme = mode)) },
                        )
                        Text(label, fontSize = 14.sp)
                    }
                }

                Text("Шрифт", fontSize = 13.sp, color = colors.dimText,
                    modifier = Modifier.padding(top = 12.dp))
                AppSettings.FONTS.forEach { font ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .selectable(
                                selected = settings.fontFamily == font,
                                onClick = { vm.updateSettings(settings.copy(fontFamily = font)) },
                            )
                            .padding(vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = settings.fontFamily == font,
                            onClick = { vm.updateSettings(settings.copy(fontFamily = font)) },
                        )
                        Text(font, fontSize = 14.sp)
                    }
                }

                Text(
                    "Размер шрифта: ${settings.fontSize.toInt()}",
                    fontSize = 13.sp,
                    color = colors.dimText,
                    modifier = Modifier.padding(top = 12.dp),
                )
                Slider(
                    value = settings.fontSize,
                    onValueChange = { vm.updateSettings(settings.copy(fontSize = it)) },
                    valueRange = 10f..28f,
                    steps = 17,
                )

                Text(
                    "Междустрочный интервал: ${"%.1f".format(settings.lineSpacing)}",
                    fontSize = 13.sp,
                    color = colors.dimText,
                )
                Slider(
                    value = settings.lineSpacing,
                    onValueChange = { vm.updateSettings(settings.copy(lineSpacing = it)) },
                    valueRange = 1.0f..2.5f,
                    steps = 14,
                )

                // Версия из git-тега: «0.1.0» у сборки с тега, «0.1.0-3-g1a2b3c4» —
                // у промежуточной, суффикс «-dirty» — правки, которых нет в коммите
                Text(
                    "Neurodoc Editor ${BuildConfig.VERSION_NAME}",
                    fontSize = 12.sp,
                    color = colors.dimText,
                    modifier = Modifier.padding(top = 16.dp),
                )
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Готово") } },
    )
}

/** Поиск и замена: область — текущий файл или весь проект. */
@Composable
fun ReplaceDialog(vm: EditorViewModel, wholeProjectDefault: Boolean, onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    var query by remember { mutableStateOf("") }
    var replacement by remember { mutableStateOf("") }
    var wholeProject by remember { mutableStateOf(wholeProjectDefault) }
    var status by remember { mutableStateOf<String?>(null) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Поиск и замена") },
        text = {
            Column {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    label = { Text("Найти") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = replacement,
                    onValueChange = { replacement = it },
                    label = { Text("Заменить на") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                Text("Область поиска", fontSize = 13.sp, color = colors.dimText,
                    modifier = Modifier.padding(top = 12.dp))
                listOf(false to "Текущий файл", true to "Весь проект").forEach { (value, label) ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .selectable(selected = wholeProject == value, onClick = { wholeProject = value })
                            .padding(vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = wholeProject == value, onClick = { wholeProject = value })
                        Text(label, fontSize = 14.sp)
                    }
                }
                status?.let {
                    Text(it, fontSize = 13.sp, color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 8.dp))
                }
            }
        },
        confirmButton = {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TextButton(onClick = {
                    if (query.isEmpty()) {
                        status = "Укажите строку поиска."
                    } else {
                        val replaced = if (wholeProject) {
                            vm.replaceAllInProject(query, replacement) > 0
                        } else {
                            vm.replaceOneInCurrentFile(query, replacement)
                        }
                        status = if (replaced) "Заменено одно вхождение." else "Совпадений не найдено."
                    }
                }) { Text("Заменить") }
                TextButton(onClick = {
                    if (query.isEmpty()) {
                        status = "Укажите строку поиска."
                    } else {
                        val count = if (wholeProject) vm.replaceAllInProject(query, replacement)
                        else vm.replaceAllInCurrentFile(query, replacement)
                        status = if (count > 0) "Заменено вхождений: $count." else "Совпадений не найдено."
                    }
                }) { Text("Заменить всё") }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Закрыть") } },
    )
}

/** Переход к файлу по UUID с фильтруемым списком файлов проекта. */
@Composable
fun OpenByIdDialog(vm: EditorViewModel, clipboardText: String?, onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    val catalog = vm.catalog
    val prefill = remember {
        // в буфере может лежать и готовая ссылка [текст](цель) — берём из неё цель
        val raw = clipboardText?.trim().orEmpty()
        val inside = NeurodocCatalog.LINK_REGEX.find(raw)?.groupValues?.get(2) ?: raw
        catalog?.resolveInput(inside) ?: ""
    }
    var input by remember { mutableStateOf(prefill) }
    val allFiles = remember(catalog) {
        catalog?.allIds()?.map { it to (catalog.title(it)) } ?: emptyList()
    }
    val filtered = remember(input, allFiles) {
        val q = input.trim().trim('<', '>').trim()
        if (q.isEmpty()) allFiles
        else allFiles.filter { (id, title) ->
            id.contains(q, ignoreCase = true) || title.contains(q, ignoreCase = true)
        }
    }
    // не только UUID: в списке есть и файлы в собственных форматах, они адресуются путём
    val resolved = catalog?.resolveInput(input)
    val exists = resolved != null

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Открыть файл") },
        text = {
            Column {
                OutlinedTextField(
                    value = input,
                    onValueChange = { input = it },
                    label = { Text("UUID или путь к файлу") },
                    placeholder = { Text("uuid, root или contract/auth/start.yaml") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                when {
                    input.isBlank() -> Unit
                    // разобранный UUID, которого нет, и вовсе неразбираемый ввод — разные беды
                    resolved == null -> Text(
                        NeurodocCatalog.parseId(input)?.let { "Файла $it нет в этом проекте." }
                            ?: "Это не UUID и не путь к файлу проекта.",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                    else -> Text(
                        catalog?.title(resolved).orEmpty(),
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
                Text(
                    "Файлы проекта: ${filtered.size}",
                    fontSize = 12.sp,
                    color = colors.dimText,
                    modifier = Modifier.padding(top = 10.dp),
                )
                LazyColumn(modifier = Modifier.heightIn(max = 260.dp)) {
                    items(filtered.size) { index ->
                        val (id, title) = filtered[index]
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    vm.openFile(id)
                                    onDismiss()
                                }
                                .padding(vertical = 6.dp),
                        ) {
                            Text(title, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(id, fontSize = 10.sp, fontFamily = FontFamily.Monospace,
                                color = colors.dimText, maxLines = 1)
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = exists,
                onClick = {
                    resolved?.let { vm.openFile(it) }
                    onDismiss()
                },
            ) { Text("Открыть") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Удаление файла: только файл или файл вместе со ссылками на него. */
@Composable
fun DeleteFileDialog(vm: EditorViewModel, onDismiss: () -> Unit) {
    val title = vm.catalog?.title(vm.currentId.orEmpty(), 40).orEmpty()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Удалить файл?") },
        text = {
            Column {
                Text("Файл «$title» будет удалён безвозвратно.", fontSize = 14.sp)
                Text(
                    "«Только файл» — ссылки на него останутся, но перестанут работать. " +
                        "«Файл и ссылки» — ссылки будут удалены из остальных файлов.",
                    fontSize = 12.sp,
                    color = LocalNeurodocColors.current.dimText,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        },
        confirmButton = {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TextButton(onClick = { vm.deleteCurrentFile(false); onDismiss() }) {
                    Text("Только файл")
                }
                TextButton(onClick = { vm.deleteCurrentFile(true); onDismiss() }) {
                    Text("Файл и ссылки")
                }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Краткая справка по возможностям (аналог шпаргалки F1 в десктопной версии). */
@Composable
fun HelpDialog(onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    val items = listOf(
        "Ссылка" to "[текст](uuid) или [текст](путь/к/файлу.yaml); тап — переход, " +
            "http-адреса открываются в браузере",
        "Тап по ссылке" to "переход к файлу; пройденные ссылки становятся серыми",
        "Стрелки в шапке" to "назад / вперёд по истории, домик — к root",
        "«+» в шапке" to "новая ссылка: якорем станет выделенный текст или слово под курсором",
        "Связать" to "выделить текст и связать его со ссылкой из буфера обмена",
        "Снять ссылку" to "оставить текст, убрав саму ссылку",
        "Прочитано / Далее" to "отметка о прочтении и переход к следующему непрочитанному файлу",
        "Как есть" to "показать файл ровно как на диске, со всеми служебными символами",
        "Меню ⋮" to "новый файл, открыть по UUID или пути, поиск, замена, копирование, удаление",
        "Связи файла" to "входящие ссылки, битые ссылки и файлы без входящих ссылок",
        "Кнопки форматов" to "SVG, JSON, YAML, Bash, MD, AsciiDoc, HTML, nginx",
        "Сохранение" to "автоматическое, как в IDE",
    )
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Справка") },
        text = {
            LazyColumn {
                items(items.size) { index ->
                    val (key, value) = items[index]
                    Column(modifier = Modifier.padding(vertical = 5.dp)) {
                        Text(key, fontSize = 13.sp, color = MaterialTheme.colorScheme.primary)
                        Text(value, fontSize = 13.sp, color = colors.dimText)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Закрыть") } },
    )
}

/**
 * Предложение перевести каталог на новый формат.
 *
 * Переход полный: старая форма `<uuid>` ссылкой больше не считается — до миграции
 * каталог прежнего формата выглядит пустым, без единой ссылки. Файлы меняются
 * только после явного согласия и только после резервной копии всего каталога.
 */
@Composable
fun MigrationDialog(scan: LegacyScan, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Каталог прежнего формата") },
        text = {
            Column {
                Text(
                    "Ссылок старого вида <uuid>: ${scan.links} в ${scan.files} файлах. " +
                        "Каждая станет ссылкой вида [](uuid) — с пустым текстом, " +
                        "то есть будет показан заголовок целевого файла.",
                    fontSize = 14.sp,
                )
                Text(
                    "Перед изменениями будет создана копия всего каталога.",
                    fontSize = 12.sp,
                    color = colors.dimText,
                    modifier = Modifier.padding(top = 10.dp),
                )
            }
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text("Перевести") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Не сейчас") } },
    )
}

/** Запрос доступа ко всем файлам: каталоги neurodoc лежат в общей памяти устройства. */
@Composable
fun StoragePermissionDialog(onGrant: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Нужен доступ к файлам") },
        text = {
            Text(
                "Каталоги neurodoc лежат в памяти устройства, поэтому приложению нужен " +
                    "доступ ко всем файлам. Включите его в открывшихся настройках Android.",
                fontSize = 14.sp,
            )
        },
        confirmButton = { TextButton(onClick = onGrant) { Text("Открыть настройки") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Позже") } },
    )
}

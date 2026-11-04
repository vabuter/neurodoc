package today.vab.neurodoceditor.ui

import android.webkit.WebView
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import today.vab.neurodoceditor.data.GitAccount
import today.vab.neurodoceditor.ui.theme.LocalNeurodocColors
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Какая под-форма открыта в git-диалоге. */
private enum class GitPane { MAIN, CREATE_BRANCH, DELETE_BRANCH, PUBLISH, CLONE, SETTINGS }

/**
 * Git: сохранение и получение изменений, ветки, история, публикация и клонирование.
 * Как и ProjectsDialog, под-формы живут внутри одного диалога.
 */
@Composable
fun GitDialog(vm: EditorViewModel, onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    var pane by remember { mutableStateOf(GitPane.MAIN) }
    var branchToDelete by remember { mutableStateOf<String?>(null) }

    when (pane) {
        GitPane.CREATE_BRANCH -> {
            GitTextDialog(
                title = "Создать ветку",
                label = "Имя ветки",
                confirm = "Создать",
                onConfirm = { name ->
                    val bad = name.any { it.isWhitespace() } ||
                        name.any { it in "~^:?*[\\" }
                    if (name.isBlank() || bad) {
                        vm.gitError = "Имя ветки не должно быть пустым и содержать " +
                            "пробелы и символы ~ ^ : ? * [ \\"
                    } else {
                        vm.gitCreateBranch(name.trim())
                        onDismiss()
                    }
                },
                onDismiss = { pane = GitPane.MAIN },
            )
            return
        }
        GitPane.DELETE_BRANCH -> {
            val name = branchToDelete
            if (name == null) {
                pane = GitPane.MAIN
            } else {
                AlertDialog(
                    onDismissRequest = { pane = GitPane.MAIN },
                    title = { Text("Удалить ветку?") },
                    text = { Text("Ветка «$name» будет удалена и локально, и на сервере.") },
                    confirmButton = {
                        TextButton(onClick = {
                            vm.gitDeleteBranch(name)
                            onDismiss()
                        }) { Text("Удалить") }
                    },
                    dismissButton = {
                        TextButton(onClick = { pane = GitPane.MAIN }) { Text("Отмена") }
                    },
                )
            }
            return
        }
        GitPane.PUBLISH -> {
            GitTextDialog(
                title = "Опубликовать проект",
                label = "Адрес пустого репозитория (HTTPS)",
                confirm = "Опубликовать",
                onConfirm = { url ->
                    if (url.isNotBlank()) {
                        vm.gitPublish(url.trim())
                        onDismiss()
                    }
                },
                onDismiss = { pane = GitPane.MAIN },
            )
            return
        }
        GitPane.CLONE -> {
            GitCloneDialog(vm, onDone = onDismiss, onBack = { pane = GitPane.MAIN })
            return
        }
        GitPane.SETTINGS -> {
            GitSettingsDialog(vm, onDismiss = { pane = GitPane.MAIN })
            return
        }
        GitPane.MAIN -> {}
    }

    val repo = vm.git != null
    val busy = vm.gitBusy

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Git" + (vm.gitBranch?.let { " · $it" } ?: "")) },
        text = {
            Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                if (!repo) {
                    Text(
                        "Каталог проекта пока не связан с git-репозиторием.",
                        fontSize = 13.sp, color = colors.dimText,
                    )
                }
                if (repo) {
                    GitAction("Сохранить на сервере", enabled = !busy) {
                        vm.gitSave(); onDismiss()
                    }
                    GitAction("Получить с сервера", enabled = !busy) {
                        vm.gitPull(); onDismiss()
                    }
                    GitAction("Синхронизировать", enabled = !busy) {
                        vm.gitSync(); onDismiss()
                    }
                    GitAction("История коммитов", enabled = !busy) {
                        vm.gitShowHistory(); onDismiss()
                    }

                    Text(
                        "Ветки", fontSize = 13.sp, color = colors.dimText,
                        modifier = Modifier.padding(top = 12.dp, bottom = 2.dp),
                    )
                    vm.gitBranches.forEach { branch ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable(enabled = !busy && branch != vm.gitBranch) {
                                    vm.gitSwitchBranch(branch)
                                    onDismiss()
                                },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(
                                selected = branch == vm.gitBranch,
                                onClick = null,
                            )
                            Text(branch, fontSize = 14.sp, modifier = Modifier.weight(1f))
                            if (branch != vm.gitDefaultBranch) {
                                IconButton(
                                    enabled = !busy,
                                    onClick = { branchToDelete = branch; pane = GitPane.DELETE_BRANCH },
                                ) {
                                    Icon(Icons.Default.Delete, contentDescription = "Удалить ветку")
                                }
                            }
                        }
                    }
                    GitAction("Новая ветка…", enabled = !busy) { pane = GitPane.CREATE_BRANCH }
                }

                Text(
                    "Сервер", fontSize = 13.sp, color = colors.dimText,
                    modifier = Modifier.padding(top = 12.dp, bottom = 2.dp),
                )
                if (!repo) {
                    GitAction("Опубликовать проект…", enabled = !busy && vm.catalog != null) {
                        pane = GitPane.PUBLISH
                    }
                }
                GitAction("Клонировать с сервера…", enabled = !busy) { pane = GitPane.CLONE }
                GitAction("Настройки Git…", enabled = true) { pane = GitPane.SETTINGS }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Закрыть") } },
    )
}

@Composable
private fun GitAction(text: String, enabled: Boolean, onClick: () -> Unit) {
    Text(
        text,
        fontSize = 14.sp,
        color = if (enabled) MaterialTheme.colorScheme.primary
        else LocalNeurodocColors.current.dimText,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, onClick = onClick)
            .padding(vertical = 9.dp),
    )
}

/** Диалог с одним текстовым полем. */
@Composable
private fun GitTextDialog(
    title: String,
    label: String,
    confirm: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var value by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = value,
                onValueChange = { value = it },
                label = { Text(label) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = { TextButton(onClick = { onConfirm(value) }) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Клонирование: адрес, название, папка. Непустая папка — явное предупреждение. */
@Composable
private fun GitCloneDialog(vm: EditorViewModel, onDone: () -> Unit, onBack: () -> Unit) {
    val colors = LocalNeurodocColors.current
    var url by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var nameEdited by remember { mutableStateOf(false) }
    var path by remember { mutableStateOf("") }
    var confirmClear by remember { mutableStateOf(false) }

    fun start() {
        if (url.isBlank() || name.isBlank() || path.isBlank()) {
            vm.gitError = "Заполните адрес, название и папку."
            return
        }
        val dir = java.io.File(path.trim())
        if (dir.isDirectory && dir.listFiles()?.isNotEmpty() == true && !confirmClear) {
            confirmClear = true
            return
        }
        vm.gitClone(url.trim(), name.trim(), path.trim())
        onDone()
    }

    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text("Папка не пуста") },
            text = {
                Text(
                    "В папке «${path.trim()}» уже есть файлы. Они будут удалены, " +
                        "и репозиторий будет клонирован с сервера как есть."
                )
            },
            confirmButton = {
                TextButton(onClick = { start() }) { Text("Удалить и клонировать") }
            },
            dismissButton = {
                TextButton(onClick = { confirmClear = false }) { Text("Назад") }
            },
        )
        return
    }

    AlertDialog(
        onDismissRequest = onBack,
        title = { Text("Клонировать с сервера") },
        text = {
            Column {
                OutlinedTextField(
                    value = url,
                    onValueChange = {
                        url = it
                        if (!nameEdited) {
                            // название проекта подставляется из адреса, пока его не правили
                            val tail = it.trim().trimEnd('/').substringAfterLast('/')
                            name = tail.removeSuffix(".git")
                        }
                    },
                    label = { Text("Адрес репозитория (HTTPS)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it; nameEdited = true },
                    label = { Text("Название проекта") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                OutlinedTextField(
                    value = path,
                    onValueChange = { path = it },
                    label = { Text("Папка на устройстве") },
                    placeholder = { Text("/storage/emulated/0/Develop/project") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                Text(
                    "Для приватных репозиториев задайте токен в «Настройки Git…».",
                    fontSize = 12.sp, color = colors.dimText,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        },
        confirmButton = { TextButton(onClick = { start() }) { Text("Клонировать") } },
        dismissButton = { TextButton(onClick = onBack) { Text("Отмена") } },
    )
}

/** Автор коммитов и токены серверов. Токены хранятся в приватной папке приложения. */
@Composable
private fun GitSettingsDialog(vm: EditorViewModel, onDismiss: () -> Unit) {
    val colors = LocalNeurodocColors.current
    val settings = vm.settings
    var userName by remember { mutableStateOf(settings.gitUserName) }
    var userEmail by remember { mutableStateOf(settings.gitUserEmail) }
    var githubToken by remember {
        mutableStateOf(settings.gitAccounts.firstOrNull { it.host == "github.com" }?.token ?: "")
    }
    var gitlabToken by remember {
        mutableStateOf(settings.gitAccounts.firstOrNull { it.host == "gitlab.com" }?.token ?: "")
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Настройки Git") },
        text = {
            Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                OutlinedTextField(
                    value = userName,
                    onValueChange = { userName = it },
                    label = { Text("Имя автора коммитов") },
                    placeholder = { Text(android.os.Build.MODEL) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = userEmail,
                    onValueChange = { userEmail = it },
                    label = { Text("Почта автора коммитов") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                OutlinedTextField(
                    value = githubToken,
                    onValueChange = { githubToken = it },
                    label = { Text("Токен github.com") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 14.dp),
                )
                OutlinedTextField(
                    value = gitlabToken,
                    onValueChange = { gitlabToken = it },
                    label = { Text("Токен gitlab.com") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                Text(
                    "Токен создаётся на сайте сервера (Personal Access Token, право " +
                        "чтения и записи репозитория) и хранится только на этом устройстве.",
                    fontSize = 12.sp, color = colors.dimText,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val accounts = buildList {
                    if (githubToken.isNotBlank())
                        add(GitAccount(host = "github.com", token = githubToken.trim()))
                    if (gitlabToken.isNotBlank())
                        add(GitAccount(host = "gitlab.com", token = gitlabToken.trim()))
                }
                vm.updateSettings(
                    settings.copy(
                        gitUserName = userName.trim(),
                        gitUserEmail = userEmail.trim(),
                        gitAccounts = accounts,
                    )
                )
                onDismiss()
            }) { Text("Сохранить") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Ошибка git: полный текст, в Toast он не помещается. */
@Composable
fun GitErrorDialog(message: String, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Ошибка Git") },
        text = {
            Text(
                message,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.verticalScroll(rememberScrollState()),
            )
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Закрыть") } },
    )
}

/**
 * Конфликт с сервером. PUSH и PULL предлагают по одному перезатиранию,
 * SYNC — оба; отмена оставляет всё как есть.
 */
@Composable
fun GitConflictDialog(kind: GitConflictKind, vm: EditorViewModel, onDismiss: () -> Unit) {
    val text = when (kind) {
        GitConflictKind.PUSH ->
            "На сервере есть изменения, которых нет у вас.\n\nМожно перезаписать " +
                "серверную версию своей — серверные изменения будут потеряны. Либо " +
                "отмените операцию и разрешите конфликт в специализированном git-приложении."
        GitConflictKind.PULL ->
            "Серверные изменения конфликтуют с вашими.\n\nМожно взять серверную версию — " +
                "ваши локальные изменения будут потеряны. Либо отмените операцию и " +
                "разрешите конфликт в специализированном git-приложении."
        GitConflictKind.SYNC ->
            "Ваши изменения и серверные конфликтуют.\n\n«Взять серверные» — ваши " +
                "изменения будут потеряны. «Взять мои» — серверные будут потеряны. Либо " +
                "отмените операцию и разрешите конфликт в специализированном git-приложении."
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Конфликт с сервером") },
        text = { Text(text, fontSize = 14.sp) },
        confirmButton = {
            when (kind) {
                GitConflictKind.PUSH -> TextButton(onClick = {
                    vm.gitResolveTakeMine(); onDismiss()
                }) { Text("Мои затирают серверные") }
                GitConflictKind.PULL -> TextButton(onClick = {
                    vm.gitResolveTakeServer(); onDismiss()
                }) { Text("Серверные затирают мои") }
                GitConflictKind.SYNC -> Row {
                    TextButton(onClick = {
                        vm.gitResolveTakeServer(); onDismiss()
                    }) { Text("Взять серверные") }
                    TextButton(onClick = {
                        vm.gitResolveTakeMine(); onDismiss()
                    }) { Text("Взять мои") }
                }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Панель истории: коммиты текущей ветки, файлы выбранного коммита, дельты. */
@Composable
fun GitHistoryPanel(vm: EditorViewModel) {
    val colors = LocalNeurodocColors.current
    Surface(
        color = colors.paneBackground,
        modifier = Modifier.fillMaxWidth().height(320.dp),
    ) {
        Column(modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp)) {
            val commit = vm.gitSelectedCommit
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (commit != null) {
                    TextButton(onClick = { vm.gitBackToCommits() }) { Text("← Коммиты") }
                }
                Text(
                    text = when {
                        commit == null -> "История · " + (vm.gitBranch ?: "?")
                        vm.gitSinceMode -> "С коммита " + commit.shortHash
                        else -> "Коммит " + commit.shortHash
                    },
                    fontSize = 14.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = { vm.gitHideHistory() }) {
                    Icon(Icons.Default.Close, contentDescription = "Закрыть")
                }
            }

            if (commit == null) {
                if (vm.gitCommits.isEmpty()) {
                    Text("Коммитов пока нет", fontSize = 12.sp, color = colors.dimText)
                }
                LazyColumn(modifier = Modifier.fillMaxWidth()) {
                    items(vm.gitCommits.size) { index ->
                        val item = vm.gitCommits[index]
                        val stamp = SimpleDateFormat("dd.MM.yyyy HH:mm", Locale.US)
                            .format(Date(item.dateMillis))
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { vm.gitSelectCommit(item) }
                                .padding(vertical = 6.dp),
                        ) {
                            Text(item.message, fontSize = 13.sp, maxLines = 1,
                                overflow = TextOverflow.Ellipsis)
                            Text(
                                "${item.shortHash}  $stamp  ${item.author}",
                                fontSize = 10.sp, fontFamily = FontFamily.Monospace,
                                color = colors.dimText, maxLines = 1,
                            )
                        }
                    }
                }
            } else {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { vm.gitSetSinceMode(!vm.gitSinceMode) },
                ) {
                    Checkbox(
                        checked = vm.gitSinceMode,
                        onCheckedChange = { vm.gitSetSinceMode(it) },
                    )
                    Text("Все изменения с этого коммита", fontSize = 13.sp)
                }
                if (vm.gitFiles.isEmpty()) {
                    Text("Изменённых файлов нет", fontSize = 12.sp, color = colors.dimText)
                }
                LazyColumn(modifier = Modifier.fillMaxWidth()) {
                    items(vm.gitFiles.size) { index ->
                        val row = vm.gitFiles[index]
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(
                                modifier = Modifier
                                    .weight(1f)
                                    .clickable { vm.gitOpenFile(row) }
                                    .padding(vertical = 6.dp),
                            ) {
                                Text(row.title, fontSize = 13.sp, maxLines = 1,
                                    overflow = TextOverflow.Ellipsis)
                                Text(row.detail, fontSize = 10.sp,
                                    fontFamily = FontFamily.Monospace,
                                    color = colors.dimText, maxLines = 1,
                                    overflow = TextOverflow.Ellipsis)
                            }
                            TextButton(onClick = { vm.gitShowDiff(row) }) { Text("±") }
                        }
                    }
                }
            }
        }
    }
}

/** Дельта файла на месте редактора; закрывается крестиком или открытием файла. */
@Composable
fun GitDiffViewer(vm: EditorViewModel) {
    val colors = LocalNeurodocColors.current
    val dark = MaterialTheme.colorScheme.background.luminance() < 0.5f
    val diff = vm.gitDiff ?: return
    val html = remember(diff, dark, vm.settings.fontFamily, vm.settings.fontSize) {
        buildDiffHtml(diff.first, diff.second, dark, vm.settings.fontSize)
    }
    val background = colors.editorBackground.toArgb()

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "Что изменилось",
                fontSize = 13.sp,
                color = colors.dimText,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = { vm.gitDiff = null }) {
                Icon(Icons.Default.Close, contentDescription = "Закрыть дельту")
            }
        }
        Surface(
            modifier = Modifier.fillMaxSize().padding(horizontal = 10.dp),
            color = colors.editorBackground,
            shape = RoundedCornerShape(10.dp),
        ) {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { ctx ->
                    WebView(ctx).apply {
                        settings.javaScriptEnabled = false
                        setBackgroundColor(background)
                    }
                },
                update = { webView ->
                    webView.setBackgroundColor(background)
                    webView.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null)
                },
            )
        }
    }
}

/**
 * HTML для unified diff: зелёные добавленные строки, красные удалённые.
 * Повторяет BuildDiffHtml из MainWindow.Viewer.cs — расходиться им нельзя.
 */
private fun buildDiffHtml(title: String, diff: String, dark: Boolean, fontSize: Float): String {
    val background = if (dark) "#1a1a1a" else "#ffffff"
    val foreground = if (dark) "#e8e8e8" else "#1b1b1b"
    val addBg = if (dark) "#1c2f22" else "#e6ffec"
    val delBg = if (dark) "#3a2124" else "#ffebe9"
    val dim = if (dark) "#8a8a8a" else "#6e6e6e"

    fun escape(s: String) = s
        .replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    val body = StringBuilder()
    var empty = true
    for (raw in diff.split("\n")) {
        val line = raw.trimEnd('\r')
        if (line.isEmpty() && empty) continue
        val cls = when {
            line.startsWith("+++") || line.startsWith("---") -> "meta"
            line.startsWith("diff ") || line.startsWith("index ") ||
                line.startsWith("new file") || line.startsWith("deleted file") ||
                line.startsWith("rename ") || line.startsWith("similarity ") -> "meta"
            line.startsWith("@@") -> "hunk"
            line.startsWith("+") -> "add"
            line.startsWith("-") -> "del"
            else -> "ctx"
        }
        val escaped = escape(line)
        body.append("<div class='").append(cls).append("'>")
            .append(if (escaped.isEmpty()) "&nbsp;" else escaped).append("</div>")
        empty = false
    }
    if (empty) body.append("<div class='meta'>Изменений нет</div>")

    return "<!DOCTYPE html><html><head><meta charset='utf-8'>" +
        "<meta name='viewport' content='width=device-width,initial-scale=1'><style>" +
        "body{margin:0;background:$background;color:$foreground;" +
        "font-family:monospace;font-size:${fontSize}px;line-height:1.45;}" +
        "#wrap{padding:10px 14px 14px;}" +
        "#title{font-weight:600;padding:4px 0 10px;color:$foreground;}" +
        "#wrap div{white-space:pre-wrap;word-break:break-word;padding:0 6px;}" +
        ".add{background:$addBg;}" +
        ".del{background:$delBg;}" +
        ".hunk{color:$dim;padding-top:8px !important;}" +
        ".meta{color:$dim;}" +
        "</style></head><body><div id='wrap'>" +
        "<div id='title'>${escape(title)}</div>" +
        body + "</div></body></html>"
}

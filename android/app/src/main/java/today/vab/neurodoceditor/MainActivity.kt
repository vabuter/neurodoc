package today.vab.neurodoceditor

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.Settings
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.viewmodel.compose.viewModel
import today.vab.neurodoceditor.data.NeurodocCatalog
import today.vab.neurodoceditor.data.ProjectInfo
import today.vab.neurodoceditor.ui.DeleteFileDialog
import today.vab.neurodoceditor.ui.EditorScreen
import today.vab.neurodoceditor.ui.GitConflictDialog
import today.vab.neurodoceditor.ui.GitDialog
import today.vab.neurodoceditor.ui.GitErrorDialog
import today.vab.neurodoceditor.ui.EditorViewModel
import today.vab.neurodoceditor.ui.HelpDialog
import today.vab.neurodoceditor.ui.MigrationDialog
import today.vab.neurodoceditor.ui.OpenByIdDialog
import today.vab.neurodoceditor.ui.ProjectsDialog
import today.vab.neurodoceditor.ui.ReplaceDialog
import today.vab.neurodoceditor.ui.SettingsDialog
import today.vab.neurodoceditor.ui.StoragePermissionDialog
import today.vab.neurodoceditor.ui.theme.NeurodocTheme

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // JGit ищет ~/.gitconfig, а user.home на Android пуст — без этого падает FS_POSIX
        System.setProperty("user.home", filesDir.absolutePath)
        enableEdgeToEdge()
        setContent {
            val vm: EditorViewModel = viewModel()
            NeurodocTheme(themeMode = vm.settings.theme) {
                AppRoot(vm)
            }
        }
    }
}

@androidx.compose.runtime.Composable
private fun AppRoot(vm: EditorViewModel) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var showProjects by remember { mutableStateOf(false) }
    var showSettings by remember { mutableStateOf(false) }
    var showHelp by remember { mutableStateOf(false) }
    var showReplace by remember { mutableStateOf(false) }
    var showOpenById by remember { mutableStateOf(false) }
    var showDelete by remember { mutableStateOf(false) }
    var showGit by remember { mutableStateOf(false) }
    var showPermission by remember { mutableStateOf(!hasStorageAccess()) }
    var pendingProject by remember { mutableStateOf<ProjectInfo?>(null) }

    val folderPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree()
    ) { uri: Uri? ->
        val project = pendingProject
        if (uri != null && project != null) {
            val path = treeUriToPath(uri)
            if (path != null) {
                vm.addOrUpdateProject(project.copy(
                    name = project.name.ifBlank { path.substringAfterLast('/') },
                    path = path,
                ))
                vm.openProject(vm.settings.projects.first { it.id == project.id })
                showProjects = false
            } else {
                Toast.makeText(context, "Не удалось определить путь к папке", Toast.LENGTH_LONG).show()
            }
        }
        pendingProject = null
    }

    // сохраняем текст, когда приложение уходит в фон
    androidx.lifecycle.compose.LifecycleEventEffect(androidx.lifecycle.Lifecycle.Event.ON_PAUSE) {
        vm.saveNow()
    }

    // сообщения модели (отказы, итог миграции) — всплывашкой, иначе их никто не увидит
    androidx.compose.runtime.LaunchedEffect(vm.statusMessage) {
        vm.statusMessage?.let { message ->
            Toast.makeText(context, message, Toast.LENGTH_LONG).show()
            vm.statusMessage = null
        }
    }

    // открыть текущий проект при первом запуске
    androidx.compose.runtime.LaunchedEffect(vm.settings.currentProjectId, vm.catalog) {
        if (vm.catalog == null) {
            vm.settings.currentProject?.let { vm.openProject(it) }
        }
    }

    EditorScreen(
        vm = vm,
        onOpenProjects = { showProjects = true },
        onOpenSettings = { showSettings = true },
        onOpenHelp = { showHelp = true },
        onOpenReplace = { showReplace = true },
        onOpenById = { showOpenById = true },
        onDeleteRequest = { showDelete = true },
        onCopyLink = {
            // готовая ссылка формата [текст](цель) — её вставляют в другой файл
            vm.currentFileLink()?.let { link ->
                copyToClipboard(context, link)
                Toast.makeText(context, "Ссылка скопирована: $link", Toast.LENGTH_SHORT).show()
            }
        },
        onCopyContent = {
            copyToClipboard(context, vm.text.text)
            Toast.makeText(context, "Содержимое скопировано", Toast.LENGTH_SHORT).show()
        },
        onWebLink = { url ->
            runCatching {
                context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
            }.onFailure {
                Toast.makeText(context, "Не удалось открыть ссылку", Toast.LENGTH_SHORT).show()
            }
        },
        onLinkSelection = {
            // буфер читаем в момент действия: следить за ним постоянно ни к чему
            val refusal = vm.linkSelection(readClipboard(context))
            val message = refusal ?: "Текст связан со ссылкой из буфера"
            Toast.makeText(context, message, Toast.LENGTH_SHORT).show()
        },
        onOpenGit = { showGit = true },
    )

    if (showPermission) {
        StoragePermissionDialog(
            onGrant = {
                showPermission = false
                requestStorageAccess(context)
            },
            onDismiss = { showPermission = false },
        )
    }
    if (showProjects) {
        ProjectsDialog(
            vm = vm,
            onDismiss = { showProjects = false },
            onPickFolder = { project ->
                vm.addOrUpdateProject(project)
                pendingProject = project
                folderPicker.launch(null)
            },
        )
    }
    if (showSettings) {
        SettingsDialog(vm = vm, onDismiss = { showSettings = false })
    }
    if (showHelp) {
        HelpDialog(onDismiss = { showHelp = false })
    }
    if (showReplace) {
        ReplaceDialog(vm = vm, wholeProjectDefault = false, onDismiss = { showReplace = false })
    }
    if (showOpenById) {
        OpenByIdDialog(
            vm = vm,
            clipboardText = readClipboard(context),
            onDismiss = { showOpenById = false },
        )
    }
    if (showDelete) {
        DeleteFileDialog(vm = vm, onDismiss = { showDelete = false })
    }
    if (showGit) {
        GitDialog(vm = vm, onDismiss = { showGit = false })
    }
    vm.gitError?.let { message ->
        GitErrorDialog(message = message, onDismiss = { vm.gitError = null })
    }
    vm.gitConflict?.let { kind ->
        GitConflictDialog(kind = kind, vm = vm, onDismiss = { vm.gitConflict = null })
    }
    vm.pendingMigration?.let { scan ->
        MigrationDialog(
            scan = scan,
            onConfirm = { vm.runMigration() },
            onDismiss = { vm.dismissMigration() },
        )
    }
}

private fun hasStorageAccess(): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.R || Environment.isExternalStorageManager()

private fun requestStorageAccess(context: Context) {
    runCatching {
        val intent = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION)
        intent.data = Uri.parse("package:" + context.packageName)
        context.startActivity(intent)
    }.onFailure {
        context.startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
    }
}

/** Текст из буфера обмена — для подстановки UUID в диалог перехода. */
private fun readClipboard(context: Context): String? = runCatching {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString()
}.getOrNull()

private fun copyToClipboard(context: Context, text: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText("neurodoc", text))
}

/** Преобразует tree-uri из системного пикера в обычный путь файловой системы. */
private fun treeUriToPath(uri: Uri): String? {
    val docId = runCatching { DocumentsContract.getTreeDocumentId(uri) }.getOrNull() ?: return null
    val parts = docId.split(":")
    if (parts.size != 2) return null
    val (type, relative) = parts
    return when {
        type.equals("primary", ignoreCase = true) ->
            Environment.getExternalStorageDirectory().path + "/" + relative
        else -> "/storage/$type/$relative"
    }
}

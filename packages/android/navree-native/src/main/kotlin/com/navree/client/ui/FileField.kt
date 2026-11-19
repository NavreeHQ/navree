package com.navree.client.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.text.format.Formatter
import android.webkit.MimeTypeMap
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContract
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import com.navree.client.R
import com.navree.client.core.FieldValue
import com.navree.client.core.FileRef
import com.navree.client.core.ScreenView
import com.navree.client.design.FileRules
import com.navree.client.design.FormRules
import com.navree.client.platform.PickedFiles
import com.navree.protocol.Navree
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.concurrent.thread

/**
 * What the system's picker is asked for, as one contract whatever the field
 * takes: the photo picker (one picture or video, or several) when the field
 * takes only pictures and video, the system's file picker otherwise (one
 * document or several). Neither asks for a permission. Its result is the list
 * of documents chosen, none when the person left without choosing.
 */
internal class PickFiles : ActivityResultContract<FileRules.Plan, List<Uri>>() {
    private val onePhoto = ActivityResultContracts.PickVisualMedia()
    private val manyPhotos = ActivityResultContracts.PickMultipleVisualMedia()
    private val oneDocument = ActivityResultContracts.OpenDocument()
    private val manyDocuments = ActivityResultContracts.OpenMultipleDocuments()
    private var lastPlan: FileRules.Plan? = null

    override fun createIntent(context: Context, input: FileRules.Plan): Intent {
        lastPlan = input
        val photos = input.source == FileRules.Source.Photos
        return when {
            photos && input.multiple -> manyPhotos.createIntent(context, request(input))
            photos -> onePhoto.createIntent(context, request(input))
            input.multiple -> manyDocuments.createIntent(context, input.documentTypes.toTypedArray())
            else -> oneDocument.createIntent(context, input.documentTypes.toTypedArray())
        }
    }

    override fun parseResult(resultCode: Int, intent: Intent?): List<Uri> {
        val plan = lastPlan
        val photos = plan?.source == FileRules.Source.Photos
        val multiple = plan?.multiple == true
        return when {
            photos && multiple -> manyPhotos.parseResult(resultCode, intent)
            photos -> listOfNotNull(onePhoto.parseResult(resultCode, intent))
            multiple -> manyDocuments.parseResult(resultCode, intent)
            else -> listOfNotNull(oneDocument.parseResult(resultCode, intent))
        }
    }

    private fun request(plan: FileRules.Plan): PickVisualMediaRequest = PickVisualMediaRequest(
        when (val media = plan.media) {
            FileRules.Media.ImageAndVideo -> ActivityResultContracts.PickVisualMedia.ImageAndVideo
            FileRules.Media.ImageOnly -> ActivityResultContracts.PickVisualMedia.ImageOnly
            FileRules.Media.VideoOnly -> ActivityResultContracts.PickVisualMedia.VideoOnly
            is FileRules.Media.Single -> ActivityResultContracts.PickVisualMedia.SingleMimeType(media.mime)
        },
    )
}

/** The client's words for a file that was not taken. */
internal fun refusalWords(context: Context, why: FileRules.Refusal): String = when (why) {
    is FileRules.Refusal.TooLarge -> context.getString(R.string.navree_file_too_large, why.name, Formatter.formatShortFileSize(context, why.most))
    is FileRules.Refusal.TooLargeTogether -> context.getString(R.string.navree_file_too_large_together, why.name, Formatter.formatShortFileSize(context, why.most))
    is FileRules.Refusal.WrongType -> context.getString(R.string.navree_file_wrong_type, why.name, why.accepted)
    is FileRules.Refusal.Unreadable -> context.getString(R.string.navree_file_unreadable, why.name)
    is FileRules.Refusal.NoAnswer -> context.getString(R.string.navree_file_no_answer, why.name)
    is FileRules.Refusal.WithLocation -> context.getString(R.string.navree_file_with_location, why.name)
    is FileRules.Refusal.PlaceUnchecked -> context.getString(R.string.navree_file_place_unchecked, why.name)
    is FileRules.Refusal.VideoUnchecked -> context.getString(R.string.navree_file_video_unchecked, why.name)
}

/** The tokens of every file a field of any screen the core holds names. */
internal fun heldTokens(screens: Collection<ScreenView>): Set<String> =
    screens.flatMap { screen -> screen.forms.values.flatMap { it.fields.values } }
        .flatMap { (it.value as? FieldValue.Files)?.files.orEmpty() }
        .mapTo(HashSet()) { it.token }

/**
 * A file field, composed from Material 3's parts as it has no component for
 * one (its component list has none): an outlined box, as a text field is, with
 * the label, a row for each file chosen (its picture or document symbol, its
 * name and size, and a button to remove it) and a button to choose with. The
 * button opens the system's own picker; each file that comes back is copied
 * off the main thread ([PickedFiles]) and told to the core by its token.
 *
 * A file that does not fit the field is said at the field and not added. A
 * file that can no longer be read (the system cleared the copy while the app
 * was away) is shown as gone, and is sent as no file. While the form is on its
 * way the field cannot be changed, as the files it sends are being read.
 *
 * When the field leaves the composition and no screen the core still holds
 * names its files (the person left the form's screen), their copies go. A
 * field that only scrolled out of sight keeps them: its screen still names them.
 */
@Composable
internal fun FileFieldView(field: Navree.Field, input: Navree.FileInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val context = LocalContext.current
    val store = remember(context) { PickedFiles.of(context) }
    val held = (place.value(field) as? FieldValue.Files)?.files.orEmpty()
    val latestHeld by rememberUpdatedState(held)
    val latestPlace by rememberUpdatedState(place)
    val support = support(field, place)
    var notice by rememberSaveable { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val gone = rememberGone(held, store)
    val scope = rememberCoroutineScope()
    DisposableEffect(store) {
        onDispose {
            val mine = latestHeld.map { it.token }
            if (mine.isNotEmpty()) {
                // The composition is left after the core's state changed: a screen it no longer holds is gone.
                val left = mine - heldTokens(ui.driver.screens.values)
                if (left.isNotEmpty()) thread(name = "navree-picked-drop", isDaemon = true) { left.forEach(store::drop) }
            }
        }
    }
    val plan = remember(input.accept, input.multiple) {
        FileRules.plan(input.accept, input.multiple) { extension -> MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) }
    }
    val launcher = rememberLauncherForActivityResult(remember { PickFiles() }) { uris ->
        if (uris.isNotEmpty()) {
            scope.launch {
                busy = true
                val results = try {
                    // With several files the total obeys the limit too: each is checked against what the field holds and what came before it.
                    var total = if (input.multiple) latestHeld.sumOf { it.size } else 0L
                    uris.map { uri ->
                        store.keep(context.contentResolver, uri, input.accept, input.keepLocation, total).also { if (it is PickedFiles.Result.Kept) total += it.file.size }
                    }
                } finally {
                    busy = false
                }
                val refused = results.filterIsInstance<PickedFiles.Result.Refused>()
                val kept = results.filterIsInstance<PickedFiles.Result.Kept>().map { it.file }
                notice = refused.takeIf { it.isNotEmpty() }?.joinToString("\n") { refusalWords(context, it.why) }
                if (kept.isNotEmpty()) {
                    val before = latestHeld
                    // One file replaces the one held; the extras a picker gave a field that takes one are let go.
                    val next = if (input.multiple) before + kept else kept.take(1)
                    val leftOut = if (input.multiple) emptyList() else before + kept.drop(1)
                    ui.change(latestPlace, field, FieldValue.Files(next))
                    if (leftOut.isNotEmpty()) launch(Dispatchers.IO) { leftOut.forEach { store.drop(it.token) } }
                }
            }
        }
    }
    val label = field.label ?: field.name
    val open = !field.disabled && !place.pending && !busy
    val problem = notice != null || support.isError
    val colours = MaterialTheme.colorScheme

    Column(
        modifier.fillMaxWidth().semantics { if (support.isError) error(support.text.orEmpty()) },
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .border(1.dp, if (problem) colours.error else colours.outline, MaterialTheme.shapes.extraSmall)
                .padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        ) {
            FormRules.label(field.label, field.required)?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, color = if (problem) colours.error else colours.onSurfaceVariant, modifier = Modifier.padding(end = 8.dp))
            }
            if (held.isEmpty()) {
                Box(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(end = 8.dp), contentAlignment = Alignment.CenterStart) {
                    Text(stringResource(R.string.navree_file_none), style = MaterialTheme.typography.bodyLarge, color = colours.onSurfaceVariant)
                }
            }
            for (file in held) {
                FileRow(file, file.token in gone, removable = open) {
                    notice = null
                    ui.change(place, field, FieldValue.Files(held.filter { it.token != file.token }))
                    scope.launch(Dispatchers.IO) { store.drop(file.token) }
                }
            }
            if (busy) {
                val reading = stringResource(R.string.navree_file_reading)
                LinearProgressIndicator(Modifier.fillMaxWidth().padding(end = 8.dp, top = 4.dp, bottom = 4.dp).semantics { contentDescription = reading })
            }
            val choose = stringResource(
                when {
                    held.isEmpty() -> if (input.multiple) R.string.navree_choose_files else R.string.navree_choose_file
                    input.multiple -> R.string.navree_add_files
                    else -> R.string.navree_replace_file
                },
            )
            val said = stringResource(R.string.navree_file_button_for, choose, label)
            OutlinedButton(
                onClick = {
                    try {
                        launcher.launch(plan)
                    } catch (_: ActivityNotFoundException) {
                        notice = context.getString(R.string.navree_file_no_picker)
                    }
                },
                enabled = open,
                modifier = Modifier.semantics { contentDescription = said },
            ) {
                Icon(symbolVector("attach_file")!!, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(choose)
            }
        }
        notice?.let { Problem(it, Modifier.semantics { liveRegion = LiveRegionMode.Polite }) }
        if (support.isError) {
            Problem(support.text.orEmpty())
        } else {
            support.text?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = colours.onSurfaceVariant, modifier = Modifier.padding(start = 16.dp, top = 4.dp)) }
        }
    }
}

/** A line that says what is wrong, with a symbol, never by colour alone. */
@Composable
private fun Problem(text: String, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().padding(start = 16.dp, top = 4.dp).semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.Top) {
        Icon(Glyphs.Error, contentDescription = null, tint = MaterialTheme.colorScheme.error, modifier = Modifier.size(16.dp).padding(top = 2.dp))
        Spacer(Modifier.width(8.dp))
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
    }
}

/** One chosen file: its symbol, its name and size (or that it is gone), and the button that removes it. */
@Composable
private fun FileRow(file: FileRef, gone: Boolean, removable: Boolean, onRemove: () -> Unit) {
    val colours = MaterialTheme.colorScheme
    val context = LocalContext.current
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp), verticalAlignment = Alignment.CenterVertically) {
        val symbol = when {
            gone -> "warning"
            file.mediaType.startsWith("image/") -> "image"
            else -> "description"
        }
        Icon(symbolVector(symbol)!!, contentDescription = null, tint = if (gone) colours.error else colours.onSurfaceVariant, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(16.dp))
        // Name and size are read as one thing.
        Column(Modifier.weight(1f).padding(vertical = 4.dp).semantics(mergeDescendants = true) {}) {
            Text(file.name, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(
                if (gone) stringResource(R.string.navree_file_gone) else Formatter.formatShortFileSize(context, file.size),
                style = MaterialTheme.typography.bodySmall,
                color = if (gone) colours.error else colours.onSurfaceVariant,
            )
        }
        IconButton(onClick = onRemove, enabled = removable) {
            Icon(Glyphs.Close, contentDescription = stringResource(R.string.navree_remove_file, file.name))
        }
    }
}

/**
 * The tokens of the files held whose copies can no longer be read. Looked at
 * when the files held change and every time the app comes back to the front,
 * since the system may clear what an app keeps in its cache while it is away.
 */
@Composable
private fun rememberGone(held: List<FileRef>, store: PickedFiles): Set<String> {
    var gone by remember { mutableStateOf(emptySet<String>()) }
    var returns by remember { mutableIntStateOf(0) }
    val activity = LocalActivity.current
    DisposableEffect(activity) {
        val owner = activity as? LifecycleOwner
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) returns++ }
        owner?.lifecycle?.addObserver(observer)
        onDispose { owner?.lifecycle?.removeObserver(observer) }
    }
    val tokens = held.map { it.token }
    LaunchedEffect(tokens, returns) {
        gone = withContext(Dispatchers.IO) { tokens.filter { !store.readable(it) }.toSet() }
    }
    return gone
}

package com.navree.client.ui

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.DragEvent
import android.webkit.CookieManager
import android.webkit.MimeTypeMap
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebSettings
import android.webkit.WebView
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.viewinterop.AndroidView
import com.navree.client.BuildConfig
import com.navree.client.R
import com.navree.client.core.Failure
import com.navree.client.core.FileRef
import com.navree.client.design.FileRules
import com.navree.client.platform.ClientLog
import com.navree.client.platform.OriginWebClient
import com.navree.client.platform.Origins
import com.navree.client.platform.PickedFiles
import com.navree.client.platform.SiteWipe
import com.navree.client.platform.WebRules
import com.navree.client.platform.WorkerGuard
import com.navree.client.platform.guardDocuments
import com.navree.client.webViewToken
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/** The tag of a web view whose script at the start of every document could not be added: it loads nothing. */
private const val NOT_GUARDED = "not guarded"

/**
 * One pending `WebChromeClient.onShowFileChooser` request. Its callback must be
 * answered at most once on every path, whether with chosen URIs or with null,
 * and always on the main thread (`WebView`'s native callback checks the UI
 * thread).
 */
private class ChooserCall(
    val accept: String?,
    val multiple: Boolean,
    private val callback: ValueCallback<Array<Uri>>,
) {
    private val settled = AtomicBoolean(false)

    fun finish(uris: Array<Uri>?) {
        if (settled.compareAndSet(false, true)) {
            val main = Looper.getMainLooper()
            if (main == null || Looper.myLooper() == main) {
                callback.onReceiveValue(uris)
            } else {
                Handler(main).post { callback.onReceiveValue(uris) }
            }
        }
    }
}

private class ChooserSlot {
    var active: ChooserCall? = null
    val disposed = AtomicBoolean(false)
}

/**
 * What a web page is shown with, as the answers [WebPage] decides by: whether
 * the rules for a service worker's requests are in force, whether the script
 * at the start of every document was added to a web view, and whether the web
 * view holds nothing of a session that ended on the origin of the page's
 * address. [clean] only answers: a page is drawn again whenever what is owed
 * changes, and asks each time, so asking begins nothing. What has another go
 * at what an ended session's pages left is [again], which the person asks for
 * with "Try again" on a page held back for it. The app has only the real
 * ones; a test gives answers of its own, to see the page refused when one is
 * missing.
 */
internal class WebGuards(
    val workers: () -> Boolean = { WorkerGuard.ensure() },
    val documents: (WebView, Origins) -> Boolean = { view, origins -> guardDocuments(view, origins) },
    val clean: (address: String) -> Boolean = { address -> SiteWipe.shows(address) },
    val again: () -> Unit = { SiteWipe.again() },
)

/** The guards [WebPage] asks: the real ones unless a test provides others. */
internal val LocalWebGuards = staticCompositionLocalOf { WebGuards() }

/**
 * A page of the app's origin that could not be drawn natively, in the
 * system's web view inside the app's own chrome. It shares the native
 * requests' cookies (the system store is the one store), carries the
 * protocol's token at the end of its user agent so the server leaves out its
 * web chrome, and has no bridge to the app: no script interface, no file or
 * content access, no pop-ups, no mixed content. A `<input type="file">` while
 * the page is on the app's origins opens the client's own picker and passes
 * each chosen file through [PickedFiles] (with `keepLocation = false`) before
 * handing a read-only `content://` handle to the web view.
 *
 * The page stays on the app's origins, and what it loads inside it is asked
 * for only from them or from the app's web hosts, by the first address of each
 * request ([OriginWebClient], and a check at the start of every document,
 * [guardDocuments]; where a redirect then leads is the page's own policy's to
 * hold): a link the person taps is
 * handed to the core, which decides where it leads (a native screen, the
 * system browser, nothing); any other way of leaving is stopped, and a page
 * taken off the origins ends as a failed screen. The page's title reaches the
 * app bar only from a page on the app's origins. When the web view's process
 * ends, the page is loaded again once in a new web view, and after that the
 * screen says it failed, with "try again".
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
internal fun WebPage(
    url: String,
    serial: Long,
    onLink: (String) -> Unit,
    onTitle: (String) -> Unit,
    modifier: Modifier = Modifier,
    covered: Boolean = false,
) {
    val ui = LocalUi.current
    val origins = ui.origins
    val guards = LocalWebGuards.current
    val appContext = LocalContext.current.applicationContext
    val store = remember(appContext) { PickedFiles.of(appContext) }
    val link by rememberUpdatedState(onLink)
    val title by rememberUpdatedState(onTitle)
    // A new web view for each attempt: one whose process ended can no longer be used.
    var attempt by remember { mutableIntStateOf(0) }
    var loading by remember(url, serial, attempt, covered) { mutableStateOf(true) }
    var gone by remember { mutableIntStateOf(0) }
    var failed by remember(url, serial) { mutableStateOf(!origins.ours(url)) }
    // A page is not shown without its guards. The rules for a service worker's requests are asked for again
    // every time a page is made (and by "try again"); the script at the start of every document is added to
    // the web view itself, so its answer comes from the web view made for this attempt.
    val workers = remember(url, serial, attempt) { guards.workers() }
    var documents by remember(url, serial, attempt) { mutableStateOf(true) }
    // Nor while the web view still holds what the pages of a session that ended stored: asked again each time, and whenever
    // what is owed changes, so a page on the screen when a session ends goes at once, its web view with it, and one that was
    // held back is shown when the deletion is done. Asking begins nothing: a deletion that ends with this page's origin left
    // changes what is owed too, and a page that set it going each time it was drawn again would never stop.
    val owed = SiteWipe.changes
    val clean = remember(url, serial, attempt, owed) { guards.clean(url) }
    val missing = WebRules.missingGuards(workers, documents, clean)
    LaunchedEffect(url, serial, attempt, missing) {
        if (missing.isNotEmpty()) ClientLog.say("a web page was not shown: " + missing.joinToString("; "))
    }

    Box(modifier.fillMaxSize()) {
        if (covered || failed || missing.isNotEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                val again: () -> Unit = {
                    gone = 0
                    failed = !origins.ours(url)
                    // The person asks for it: another go at what an ended session's pages left begins here, and nowhere in the drawing.
                    if (!covered && !clean) guards.again()
                    attempt++
                }
                if (covered || (!failed && workers && documents && !clean)) {
                    // Held back by a locked cover over the app, or by what the pages of an ended session left.
                    FailureState(Failure.Protocol, stringResource(R.string.navree_web_held_title), stringResource(R.string.navree_web_held_text), again)
                } else {
                    FailureState(Failure.Protocol, retry = again)
                }
            }
        } else {
            key(attempt) {
                val scope = rememberCoroutineScope()
                val slot = remember { ChooserSlot() }
                val webTokens = remember { HashSet<String>() }
                val launcher = rememberLauncherForActivityResult(remember { PickFiles() }) { uris ->
                    val call = slot.active ?: return@rememberLauncherForActivityResult
                    slot.active = null
                    if (uris.isEmpty()) {
                        call.finish(null)
                        return@rememberLauncherForActivityResult
                    }
                    scope.launch {
                        try {
                            val chosen = if (call.multiple) uris else uris.take(1)
                            val kept = ArrayList<FileRef>()
                            val refused = ArrayList<PickedFiles.Result.Refused>()
                            var total = 0L
                            for (uri in chosen) {
                                if (slot.disposed.get()) break
                                when (val result = store.keep(appContext.contentResolver, uri, call.accept, keepLocation = false, held = total)) {
                                    is PickedFiles.Result.Kept -> {
                                        val dropNow = synchronized(webTokens) {
                                            if (slot.disposed.get()) {
                                                true
                                            } else {
                                                webTokens.add(result.file.token)
                                                false
                                            }
                                        }
                                        if (dropNow) {
                                            thread(name = "navree-web-picked-drop", isDaemon = true) { store.drop(result.file.token) }
                                            break
                                        }
                                        total += result.file.size
                                        kept += result.file
                                    }
                                    is PickedFiles.Result.Refused -> refused += result
                                }
                            }
                            if (slot.disposed.get()) {
                                call.finish(null)
                                return@launch
                            }
                            if (refused.isNotEmpty()) {
                                val text = refused.joinToString("\n") { refusalWords(appContext, it.why) }
                                launch { ui.snackbars.showSnackbar(text) }
                            }
                            if (kept.isEmpty()) {
                                call.finish(null)
                            } else {
                                call.finish(kept.map { store.uri(appContext, it) }.toTypedArray())
                            }
                        } finally {
                            call.finish(null)
                        }
                    }
                }
                val latestLauncher by rememberUpdatedState(launcher)
                DisposableEffect(Unit) {
                    onDispose {
                        val tokens = synchronized(webTokens) {
                            slot.disposed.set(true)
                            webTokens.toList().also { webTokens.clear() }
                        }
                        slot.active?.finish(null)
                        slot.active = null
                        if (tokens.isNotEmpty()) {
                            thread(name = "navree-web-picked-drop", isDaemon = true) { tokens.forEach(store::drop) }
                        }
                    }
                }
                AndroidView(
                    modifier = Modifier.fillMaxSize(),
                    factory = { context ->
                        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)
                        object : WebView(context) {
                            override fun onDragEvent(event: DragEvent?): Boolean = false
                        }.apply {
                            settings.javaScriptEnabled = true
                            settings.domStorageEnabled = true
                            settings.allowFileAccess = false
                            settings.allowContentAccess = false
                            settings.setSupportMultipleWindows(false)
                            settings.javaScriptCanOpenWindowsAutomatically = false
                            settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                            settings.setGeolocationEnabled(false)
                            settings.safeBrowsingEnabled = true
                            settings.userAgentString = settings.userAgentString + " " + webViewToken()
                            CookieManager.getInstance().setAcceptCookie(true)
                            // Cookies of a frame from another site are refused (the system's default, said here).
                            CookieManager.getInstance().setAcceptThirdPartyCookies(this, false)
                            // Before the first load, so it runs in every document of every frame. Without it
                            // the page is not loaded: `update` below does not load a web view that has this tag.
                            if (!guards.documents(this, origins)) tag = NOT_GUARDED
                            webViewClient = OriginWebClient(
                                origins,
                                onLink = { link(it) },
                                onLoading = { loading = it },
                                onLeft = {
                                    loading = false
                                    failed = true
                                },
                                onGone = {
                                    gone++
                                    if (WebRules.reloadAfterGone(gone)) attempt++ else failed = true
                                },
                            )
                            webChromeClient = object : WebChromeClient() {
                                override fun onReceivedTitle(view: WebView, text: String?) {
                                    val from = view.url
                                    if (from != null && origins.ours(from) && !text.isNullOrBlank() && !text.startsWith("http")) title(text)
                                }

                                override fun onShowFileChooser(
                                    webView: WebView,
                                    filePathCallback: ValueCallback<Array<Uri>>,
                                    fileChooserParams: FileChooserParams?,
                                ): Boolean {
                                    val mode = fileChooserParams?.mode
                                    val from = webView.url
                                    val supported = mode == FileChooserParams.MODE_OPEN || mode == FileChooserParams.MODE_OPEN_MULTIPLE
                                    if (from == null || !origins.ours(from) || !supported) {
                                        filePathCallback.onReceiveValue(null)
                                        return true
                                    }
                                    val accept = FileRules.chooserAccept(fileChooserParams.acceptTypes)
                                    val multiple = mode == FileChooserParams.MODE_OPEN_MULTIPLE
                                    val plan = FileRules.plan(accept, multiple) { extension ->
                                        MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension)
                                    }
                                    val call = ChooserCall(accept, multiple, filePathCallback)
                                    slot.active?.finish(null)
                                    slot.active = call
                                    try {
                                        latestLauncher.launch(plan)
                                    } catch (failure: Throwable) {
                                        if (failure is VirtualMachineError) throw failure
                                        slot.active = null
                                        call.finish(null)
                                        if (failure is ActivityNotFoundException || failure is SecurityException) {
                                            scope.launch { ui.snackbars.showSnackbar(appContext.getString(R.string.navree_file_no_picker)) }
                                        }
                                    }
                                    return true
                                }
                            }
                        }
                    },
                    update = { web ->
                        val wanted = "$serial $url"
                        if (web.tag === NOT_GUARDED) {
                            documents = false
                        } else if (web.tag != wanted && origins.ours(url)) {
                            web.tag = wanted
                            web.loadUrl(url)
                        }
                    },
                    onRelease = { web ->
                        web.stopLoading()
                        web.onPause()
                        web.destroy()
                    },
                )
            }
            if (loading) LinearProgressIndicator(Modifier.fillMaxWidth().align(Alignment.TopCenter))
        }
    }
}

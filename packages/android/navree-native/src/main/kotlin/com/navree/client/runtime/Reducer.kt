package com.navree.client.runtime

import com.navree.client.protocol.Action
import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.Back
import com.navree.client.protocol.Command
import com.navree.client.protocol.Commands
import com.navree.client.protocol.Dismiss
import com.navree.client.protocol.Envelope
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.NATIVE_CONTENT_TYPE
import com.navree.client.protocol.NavItem
import com.navree.client.protocol.Navigate
import com.navree.client.protocol.Node
import com.navree.client.protocol.Op
import com.navree.client.protocol.PROTOCOL_MAJOR
import com.navree.client.protocol.RegionNode
import com.navree.client.protocol.Present
import com.navree.client.protocol.Refresh
import com.navree.client.protocol.Request
import com.navree.client.protocol.SHELL_PATH
import com.navree.client.protocol.Screen
import com.navree.client.protocol.Shell
import com.navree.client.protocol.StackNode
import com.navree.client.protocol.Submit
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.UnknownAction
import com.navree.client.protocol.decodeEnvelope
import com.navree.client.protocol.decodeShell

/**
 * The client's behaviour: the state and an event give the next state and what
 * to do about it. It fetches nothing, draws nothing and keeps nothing of its
 * own, so every rule here can be checked with plain values.
 *
 * The rules, in short:
 * - One origin. An address is resolved before anything is done with it; what
 *   leaves the origin opens outside the app and what cannot be read is dropped.
 * - A stack of screens per top-level destination. A link to a destination
 *   selects it. Back pops, then returns to the first destination, then leaves.
 * - A new screen shows the last tree for its address at once and fetches a
 *   fresh one. A build the client has not seen drops every cached tree.
 * - A post that succeeds makes every other screen out of date; each is fetched
 *   again when it comes to the front.
 * - Where a post leads: out of a presented screen and back to what is under
 *   it; back to a screen already in the stack; or in place of the form.
 */
object Reducer {
    fun reduce(state: ClientState, event: Event): Step {
        val work = Work(state)
        when (event) {
            Event.Started -> start(work)
            is Event.Answered -> answered(work, event)
            is Event.DestinationChosen -> choose(work, event.index)
            is Event.Acted -> run(work, event.entry, event.action, event.source)
            is Event.Pressed -> press(work, event)
            is Event.ConfirmAnswered -> confirm(work, event)
            Event.BackPressed -> back(work)
            is Event.ModalDismissed -> {
                work.state = work.state.copy(modals = work.state.modals.filterNot { modal -> modal.holds(event.entry) })
                ensureFront(work)
            }
            is Event.OverlayToggled -> setOpen(work, event.entry, event.id, event.open)
            is Event.FieldChanged -> work.local(event.entry) { local ->
                val form = event.form ?: ""
                val errors = local.errors[form]?.minus(event.name)
                local.copy(
                    values = local.values + (form to (local.values[form].orEmpty() + (event.name to event.value))),
                    errors = if (errors == null) local.errors else local.errors + (form to errors),
                )
            }
            is Event.SearchChanged -> work.local(event.entry) { it.copy(search = event.query) }
            is Event.SearchRun -> search(work, event.entry)
            is Event.Refreshed -> refetch(work, event.entry, byUser = true)
            is Event.EndReached -> more(work, event)
            is Event.MessageShown ->
                work.state = work.state.copy(messages = work.state.messages.filterNot { it.id == event.id })
            is Event.MessageActed -> messageActed(work, event.id)
            is Event.LinkFollowed -> follow(work, event)
            is Event.Opened -> opened(work, event.url)
            is Event.FocusTaken -> work.local(event.entry) { local ->
                if (local.focus?.serial == event.serial) local.copy(focus = null) else local
            }
        }
        return work.step()
    }

    // The state in the making. It lives for one call of `reduce`.

    private class Work(var state: ClientState) {
        val effects = mutableListOf<Effect>()

        fun next(): Long {
            state = state.copy(serial = state.serial + 1)
            return state.serial
        }

        fun step(): Step = Step(state, effects.toList())

        fun update(key: Long, change: (Entry) -> Entry) {
            state = state.copy(
                tabs = state.tabs.map { tab ->
                    if (tab.stack.any { it.key == key }) tab.copy(stack = tab.stack.map { if (it.key == key) change(it) else it }) else tab
                },
                modals = state.modals.map { modal ->
                    if (modal.holds(key)) modal.copy(stack = modal.stack.map { if (it.key == key) change(it) else it }) else modal
                },
            )
        }

        fun local(key: Long, change: (Local) -> Local) = update(key) { it.copy(local = change(it.local)) }

        /** Changes the stack that holds `key`: a destination's or a modal's. */
        fun stack(key: Long, change: (List<Entry>) -> List<Entry>) {
            state = state.copy(
                tabs = state.tabs.map { tab -> if (tab.stack.any { it.key == key }) tab.copy(stack = change(tab.stack)) else tab },
                modals = state.modals.mapNotNull { modal ->
                    if (modal.holds(key)) change(modal.stack).takeIf { it.isNotEmpty() }?.let { modal.copy(stack = it) } else modal
                },
            )
        }

        fun fetch(pending: Pending, call: HttpRequest) {
            val request = next()
            state = state.copy(pending = state.pending + (request to pending))
            effects += Effect.Fetch(request, call)
        }

        fun say(message: Message) {
            state = state.copy(messages = state.messages + message)
        }
    }

    private fun Modal.holds(key: Long): Boolean = stack.any { it.key == key }

    // Starting, and the shell.

    private fun start(work: Work) {
        if (work.state.shellLoad == Load.Loading) return
        work.state = work.state.copy(shellLoad = Load.Loading)
        work.fetch(Pending.ShellLoad, HttpRequest("GET", SHELL_PATH))
    }

    private fun shellAnswered(work: Work, response: HttpResponse) {
        val had = work.state.shell != null
        fun failed(failure: Failure) {
            // A shell that is already drawn stays; only a first load can fail the app.
            work.state = work.state.copy(shellLoad = if (had) Load.Ready else Load.Failed(failure))
        }
        when (response) {
            is HttpResponse.Failed -> failed(response.failure)
            is HttpResponse.Left -> failed(Failure.Protocol)
            is HttpResponse.Answer -> when {
                response.status >= 500 -> failed(Failure.Server)
                response.status in 200..299 && mediaType(response.contentType) == NATIVE_CONTENT_TYPE -> {
                    val shell = try {
                        decodeShell(response.body)
                    } catch (_: IllegalArgumentException) {
                        null
                    }
                    when {
                        shell == null || shell.navree != PROTOCOL_MAJOR -> failed(Failure.Protocol)
                        shell.upgrade != null -> work.state = work.state.copy(upgrade = shell.upgrade, shellLoad = Load.Ready)
                        else -> install(work, shell)
                    }
                }
                // A server that has no native shell still has pages: one destination, the home page.
                else -> install(work, Shell())
            }
        }
    }

    private fun install(work: Work, shell: Shell) {
        val state = work.state
        val items = shell.nav.groups.flatMap { it.items }
            .filter { state.origin.resolve(it.href) is Target.Inside }
            .ifEmpty { listOf(NavItem(label = shell.name, href = "/")) }
        val old = state.tabs.associateBy { it.item.href }
        val tabs = items.map { item -> Tab(item, old[item.href]?.stack.orEmpty()) }
        val was = state.tabs.getOrNull(state.selected)?.item?.href
        val selected = tabs.indexOfFirst { it.item.href == was }.takeIf { it >= 0 } ?: 0
        val newBuild = shell.build != null && state.build != null && shell.build != state.build
        work.state = state.copy(
            shell = shell,
            shellLoad = Load.Ready,
            build = shell.build ?: state.build,
            tabs = tabs,
            selected = selected,
            cache = if (newBuild) state.cache.clear() else state.cache,
        )
        val launch = work.state.launch
        if (launch != null) {
            work.state = work.state.copy(launch = null)
            opened(work, launch)
        }
        ensureFront(work)
    }

    /** The address a destination opens at. */
    private fun rootUrl(state: ClientState, tab: Tab): String =
        (state.origin.resolve(tab.item.href) as? Target.Inside)?.url ?: "/"

    /** Makes sure the screen in front exists and is not out of date. */
    private fun ensureFront(work: Work) {
        val state = work.state
        val front = state.front
        if (front != null) {
            if (front.stale && !front.loading) refetch(work, front.key)
            return
        }
        val tab = state.tabs.getOrNull(state.selected) ?: return
        val entry = open(work, rootUrl(state, tab))
        work.state = work.state.copy(
            tabs = work.state.tabs.mapIndexed { index, it -> if (index == state.selected) it.copy(stack = listOf(entry)) else it },
        )
    }

    // Opening and fetching screens.

    private fun chrome(state: ClientState, url: String): Boolean =
        state.shell?.routes?.let { Routes.find(it, url) }?.chrome ?: true

    /** A new entry for `url`, showing the cached tree if there is one, with its fetch on the way. */
    private fun open(work: Work, url: String): Entry {
        val key = work.next()
        val cached = work.state.cache[url]
        work.fetch(Pending.Page(key), HttpRequest("GET", url))
        return Entry(
            key = key,
            url = url,
            content = cached?.let { Content.Native(it) } ?: Content.Blank,
            loading = true,
            chrome = chrome(work.state, url),
        )
    }

    /** An entry that already has what it shows. */
    private fun arrived(work: Work, url: String, content: Content): Entry {
        val key = work.next()
        if (content is Content.Native) work.state = work.state.copy(cache = work.state.cache.put(url, content.screen))
        return Entry(key = key, url = url, content = content, chrome = chrome(work.state, url))
    }

    /** Fetches the entry's page again. An answer still on its way for it is no longer wanted. */
    private fun refetch(work: Work, key: Long, byUser: Boolean = false) {
        val entry = work.state.entry(key) ?: return
        work.state = work.state.copy(pending = work.state.pending.filterValues { !(it is Pending.Page && it.entry == key) })
        work.update(key) { it.copy(loading = true, refreshing = byUser, failure = null, stale = false) }
        work.fetch(Pending.Page(key), HttpRequest("GET", entry.url))
    }

    // Answers.

    private sealed interface Reply {
        data class Tree(val url: String, val status: Int, val envelope: Envelope) : Reply
        data class Page(val url: String) : Reply
        data class Left(val url: String) : Reply
        data class Failed(val failure: Failure) : Reply
    }

    private fun mediaType(contentType: String?): String? = contentType?.substringBefore(';')?.trim()?.lowercase()

    private fun read(response: HttpResponse): Reply = when (response) {
        is HttpResponse.Failed -> Reply.Failed(response.failure)
        is HttpResponse.Left -> Reply.Left(response.url)
        is HttpResponse.Answer -> {
            val type = mediaType(response.contentType)
            when {
                type == NATIVE_CONTENT_TYPE -> {
                    val envelope = try {
                        decodeEnvelope(response.body)
                    } catch (_: IllegalArgumentException) {
                        null
                    }
                    if (envelope == null || envelope.navree != PROTOCOL_MAJOR) {
                        Reply.Failed(Failure.Protocol)
                    } else {
                        Reply.Tree(response.url, response.status, envelope)
                    }
                }
                response.status >= 500 -> Reply.Failed(Failure.Server)
                response.status in 200..299 && response.body.isBlank() -> Reply.Tree(response.url, response.status, Envelope())
                type == "text/html" || type == "application/xhtml+xml" -> Reply.Page(response.url)
                else -> Reply.Failed(Failure.Protocol)
            }
        }
    }

    private fun answered(work: Work, event: Event.Answered) {
        val pending = work.state.pending[event.request] ?: return
        work.state = work.state.copy(pending = work.state.pending - event.request)
        when (pending) {
            Pending.ShellLoad -> shellAnswered(work, event.response)
            is Pending.Page -> pageAnswered(work, pending, event.response)
            is Pending.Send -> sendAnswered(work, pending, event.response)
            is Pending.More -> moreAnswered(work, pending, event.response)
            is Pending.Region -> regionAnswered(work, pending, event.response)
        }
    }

    /** Takes note of the build an answer belongs to. A new one drops the cached trees and asks for the shell again. */
    private fun noteBuild(work: Work, build: String?) {
        val state = work.state
        if (build == null || build == state.build) return
        val first = state.build == null
        work.state = state.copy(build = build, cache = if (first) state.cache else state.cache.clear())
        val asking = work.state.pending.values.any { it == Pending.ShellLoad }
        if (!first && state.shell?.build != build && !asking) {
            work.fetch(Pending.ShellLoad, HttpRequest("GET", SHELL_PATH))
        }
    }

    /** What an envelope gives an entry to show, if it gives anything. */
    private fun content(state: ClientState, url: String, envelope: Envelope): Content? {
        val web = envelope.web
        val screen = envelope.screen
        return when {
            web != null -> Content.Web((state.origin.resolve(web.url, url) as? Target.Inside)?.url ?: url, web.why)
            screen != null -> {
                val missing = Coverage.missing(screen)
                if (missing.isEmpty()) Content.Native(screen) else Content.Web(url, missing.map { "client:$it" })
            }
            else -> null
        }
    }

    /** Puts `content` in the entry. What was typed stays; errors and waiting marks go. */
    private fun show(work: Work, key: Long, url: String, content: Content, keepValues: Boolean = true) {
        val chrome = chrome(work.state, url)
        work.update(key) { old ->
            old.copy(
                url = url,
                content = content,
                loading = false,
                refreshing = false,
                failure = null,
                stale = false,
                chrome = if (url == old.url) old.chrome else chrome,
                local = old.local.copy(
                    values = if (keepValues) old.local.values else emptyMap(),
                    errors = emptyMap(),
                    busy = emptySet(),
                    loadingMore = emptySet(),
                    confirm = null,
                    regionsAsked = emptySet(),
                ),
            )
        }
        if (content is Content.Native) work.state = work.state.copy(cache = work.state.cache.put(url, content.screen))
        loadRegions(work, key)
    }

    // Regions: parts of a screen with an address of their own.

    /** Asks for every region of the entry that is to be fetched after the screen is drawn and has not been yet. */
    private fun loadRegions(work: Work, key: Long) {
        val entry = work.state.entry(key) ?: return
        val screen = entry.screen ?: return
        for (region in screen.regions()) {
            val id = region.id ?: continue
            if (region.lazy && region.children.isEmpty() && id !in entry.local.regionsAsked) fetchRegion(work, key, region)
        }
    }

    private fun fetchRegion(work: Work, key: Long, region: RegionNode) {
        val entry = work.state.entry(key) ?: return
        val id = region.id ?: return
        val target = work.state.origin.resolve(region.src, entry.url) as? Target.Inside ?: return
        work.local(key) { it.copy(regionsAsked = it.regionsAsked + id) }
        work.fetch(Pending.Region(key, id), HttpRequest("GET", target.url))
    }

    /** The answer for a region is a screen: its body becomes what the region holds. */
    private fun regionAnswered(work: Work, pending: Pending.Region, response: HttpResponse) {
        val key = pending.entry
        val entry = work.state.entry(key) ?: return
        val screen = entry.screen ?: return
        when (val reply = read(response)) {
            is Reply.Tree -> {
                val envelope = reply.envelope
                noteBuild(work, envelope.build)
                val body = envelope.screen?.body
                val missing = envelope.screen?.let(Coverage::missing).orEmpty()
                if (envelope.web != null || missing.isNotEmpty()) {
                    // A part the client cannot draw: the page is shown whole in the web view, never with a hole in it.
                    val why = envelope.web?.why ?: missing.map { "client:$it" }
                    work.update(key) { it.copy(content = Content.Web(entry.url, why), loading = false) }
                } else if (body != null) {
                    val filled = TreeOps.rewrite(screen, pending.region) { node ->
                        listOf(if (node is RegionNode) node.copy(children = listOf(body)) else node)
                    }
                    work.update(key) { it.copy(content = Content.Native(filled)) }
                    work.state = work.state.copy(cache = work.state.cache.put(entry.url, filled))
                    // What arrived may hold regions of its own.
                    loadRegions(work, key)
                }
                if (envelope.ops.isNotEmpty()) applyOps(work, key, envelope.ops, afterSend = false)
            }
            is Reply.Page -> work.update(key) { it.copy(content = Content.Web(entry.url, listOf("html")), loading = false) }
            is Reply.Failed -> work.say(Message(id = work.next(), notice = reply.failure, entry = key))
            is Reply.Left -> Unit
        }
    }

    private fun pageAnswered(work: Work, pending: Pending.Page, response: HttpResponse) {
        val key = pending.entry
        val entry = work.state.entry(key) ?: return
        when (val reply = read(response)) {
            is Reply.Failed -> {
                work.update(key) { it.copy(loading = false, refreshing = false, failure = reply.failure) }
                if (entry.content != Content.Blank) work.say(Message(id = work.next(), notice = reply.failure, entry = key))
            }
            is Reply.Left -> {
                work.effects += Effect.OpenOutside(reply.url)
                if (entry.content == Content.Blank) remove(work, key) else work.update(key) { it.copy(loading = false, refreshing = false) }
            }
            is Reply.Page -> show(work, key, reply.url, Content.Web(reply.url, listOf("html")))
            is Reply.Tree -> {
                val envelope = reply.envelope
                if (envelope.upgrade != null) {
                    work.state = work.state.copy(upgrade = envelope.upgrade)
                    work.update(key) { it.copy(loading = false, refreshing = false) }
                    return
                }
                noteBuild(work, envelope.build)
                val content = content(work.state, reply.url, envelope)
                if (content != null) {
                    show(work, key, reply.url, content)
                } else {
                    work.update(key) { it.copy(loading = false, refreshing = false) }
                }
                if (envelope.ops.isNotEmpty()) applyOps(work, key, envelope.ops, afterSend = false)
            }
        }
    }

    /** Takes an entry out of its stack. A modal with nothing left goes with it. */
    private fun remove(work: Work, key: Long) {
        work.stack(key) { stack -> stack.filterNot { it.key == key } }
        ensureFront(work)
    }

    private fun sendAnswered(work: Work, pending: Pending.Send, response: HttpResponse) {
        val key = pending.entry
        val sender = work.state.entry(key)
        if (pending.busy != null) work.local(key) { it.copy(busy = it.busy - pending.busy) }
        if (pending.inPlace) work.update(key) { it.copy(loading = false) }
        when (val reply = read(response)) {
            is Reply.Failed -> work.say(Message(id = work.next(), notice = reply.failure, entry = key))
            is Reply.Left -> work.effects += Effect.OpenOutside(reply.url)
            is Reply.Page -> {
                if (pending.write) wrote(work, key)
                arrive(work, key, reply.url, Content.Web(reply.url, listOf("html")))
            }
            is Reply.Tree -> {
                val envelope = reply.envelope
                if (envelope.upgrade != null) {
                    work.state = work.state.copy(upgrade = envelope.upgrade)
                    return
                }
                noteBuild(work, envelope.build)
                val worked = reply.status < 400
                if (pending.write && worked) {
                    wrote(work, key)
                    // A screen with no chrome is a sign-in or the like: who is signed in may have changed.
                    if (sender?.chrome == false) work.fetch(Pending.ShellLoad, HttpRequest("GET", SHELL_PATH))
                }
                var current = key
                val content = content(work.state, reply.url, envelope)
                if (content != null) {
                    if (pending.inPlace && work.state.entry(key) != null) {
                        // A query of the screen itself: the answer takes its place, and what was typed stays.
                        // A sheet the query was sent from has done its work.
                        closeOverlays(work, key)
                        show(work, key, reply.url, content)
                    } else {
                        current = arrive(work, key, reply.url, content)
                    }
                }
                if (envelope.ops.isNotEmpty()) applyOps(work, current, envelope.ops, afterSend = true)
            }
        }
    }

    /** A write happened: no cached tree can be trusted, and every other screen may show old data. */
    private fun wrote(work: Work, except: Long) {
        val state = work.state
        fun mark(stack: List<Entry>) = stack.map { if (it.key == except) it else it.copy(stale = true) }
        work.state = state.copy(
            cache = state.cache.clear(),
            tabs = state.tabs.map { it.copy(stack = mark(it.stack)) },
            modals = state.modals.map { it.copy(stack = mark(it.stack)) },
        )
    }

    /** A post was answered with a whole screen for `url`. Returns the entry that shows it. */
    private fun arrive(work: Work, key: Long, url: String, content: Content): Long {
        val entry = work.state.entry(key) ?: return key
        if (url == entry.url) {
            // The same page again, as the server now has it: its values are the ones to show.
            show(work, key, url, content, keepValues = false)
            return key
        }
        return land(work, key, url, content)
    }

    /**
     * Where a post leads when it leads to another address. `content` is what
     * the answer already brought for it; without it the address is fetched.
     */
    private fun land(work: Work, key: Long, url: String, content: Content?): Long {
        val state = work.state
        if (state.entry(key) == null) return key
        if (state.isModal(key)) {
            // The presented screen has done its work: it goes, and what is under it carries on.
            work.state = state.copy(modals = state.modals.filterNot { it.holds(key) })
            val under = work.state.front
            if (under != null && under.url == url) {
                if (content != null) show(work, under.key, url, content) else refetch(work, under.key)
                return under.key
            }
            val entry = if (content != null) arrived(work, url, content) else open(work, url)
            if (under != null) {
                work.stack(under.key) { it + entry }
            } else {
                work.state = work.state.copy(modals = work.state.modals + Modal(Present.Full, listOf(entry)))
            }
            return entry.key
        }
        val stack = state.tabs.firstOrNull { tab -> tab.stack.any { it.key == key } }?.stack.orEmpty()
        val at = stack.indexOfFirst { it.key == key }
        val below = stack.take(at.coerceAtLeast(0)).indexOfLast { it.url == url }
        if (below >= 0) {
            // That screen is already under this one: go back to it rather than stack a copy.
            val target = stack[below]
            work.stack(key) { it.take(below + 1) }
            if (content != null) show(work, target.key, url, content) else refetch(work, target.key)
            return target.key
        }
        // Otherwise the new screen takes the place of the one that posted.
        val entry = if (content != null) arrived(work, url, content) else open(work, url)
        work.stack(key) { old -> old.map { if (it.key == key) entry else it } }
        return entry.key
    }

    // Ops.

    private fun applyOps(work: Work, key: Long, ops: List<Op>, afterSend: Boolean) {
        var current = key
        val focuses = ops.any { it is Op.Focus }
        for (op in ops) {
            when (op) {
                is Op.Morph, is Op.Replace, is Op.Append, is Op.Prepend, is Op.Remove -> changeTree(work, current, op)
                is Op.Errors -> {
                    val first = op.fields.keys.firstOrNull()
                    val serial = work.next()
                    work.local(current) { local ->
                        local.copy(
                            errors = if (op.fields.isEmpty()) local.errors - op.form else local.errors + (op.form to op.fields),
                            // Without a focus of the server's own, the first field in error is brought into view.
                            focus = if (first != null && !focuses) FocusRequest(fieldTarget(op.form, first), serial) else local.focus,
                        )
                    }
                }
                is Op.Toast -> work.say(
                    Message(
                        id = work.next(),
                        tone = op.tone,
                        text = op.text,
                        title = op.title,
                        action = op.action,
                        entry = current,
                    ),
                )
                is Op.Open -> setOpen(work, current, op.target, true)
                is Op.Close -> setOpen(work, current, op.target, false)
                is Op.Focus -> {
                    val serial = work.next()
                    work.local(current) { it.copy(focus = FocusRequest(op.target, serial)) }
                }
                is Op.Url -> {
                    val base = work.state.entry(current)?.url ?: "/"
                    val target = work.state.origin.resolve(op.href, base)
                    if (target is Target.Inside) work.update(current) { it.copy(url = target.url) }
                }
                is Op.Navigate -> {
                    val base = work.state.entry(current)?.url ?: "/"
                    val target = work.state.origin.resolve(op.href, base)
                    current = when {
                        target is Target.Outside -> {
                            work.effects += Effect.OpenOutside(target.url)
                            current
                        }
                        target is Target.Inside && afterSend && work.state.entry(current) != null ->
                            land(work, current, target.url, null)
                        target is Target.Inside -> go(work, current, target.url, null) ?: current
                        else -> current
                    }
                }
                is Op.NavigateExternal -> outside(work, op.href, current)
                Op.Reload -> refetch(work, current)
                is Op.Invalidate -> {
                    val region = work.state.entry(current)?.screen?.regions()?.firstOrNull { it.id == op.target }
                    // Without a region of that name, the whole screen is what is out of date.
                    if (region != null) fetchRegion(work, current, region) else refetch(work, current)
                }
                is Op.Unknown -> Unit
            }
        }
    }

    /** The name a field goes by when focus is asked for it and it has no id of its own. */
    fun fieldTarget(form: String, name: String): String = "$form/$name"

    private fun changeTree(work: Work, key: Long, op: Op) {
        val entry = work.state.entry(key) ?: return
        val screen = entry.screen ?: return
        val next = TreeOps.apply(screen, op)
        var local = entry.local
        val gone = when (op) {
            is Op.Replace -> screen.find(op.target)
            is Op.Remove -> screen.find(op.target)
            else -> null
        }
        if (gone != null) local = forget(local, gone)
        // A node that arrives may hold regions still to be fetched, under ids asked for before.
        val arrived = when (op) {
            is Op.Morph -> op.node.ids()
            is Op.Replace -> op.node.ids()
            is Op.Append -> op.nodes.flatMap { it.ids() }.toSet()
            is Op.Prepend -> op.nodes.flatMap { it.ids() }.toSet()
            else -> emptySet()
        }
        local = local.copy(regionsAsked = local.regionsAsked - arrived)
        work.update(key) { it.copy(content = Content.Native(next), local = local) }
        work.state = work.state.copy(cache = work.state.cache.put(entry.url, next))
        loadRegions(work, key)
    }

    /** Drops what the user did under a node that is no longer there as it was. */
    private fun forget(local: Local, node: Node): Local {
        val ids = node.ids()
        val forms = node.walk().filter { it is FormNode }.mapNotNull { it.id }.toSet()
        return local.copy(
            values = local.values - forms,
            errors = local.errors - forms,
            busy = local.busy - ids,
            opened = local.opened - ids,
            closed = local.closed - ids,
            hidden = local.hidden - ids,
            disabled = local.disabled - ids,
            texts = local.texts - ids,
            hiddenValues = local.hiddenValues - ids,
            states = local.states - ids,
        )
    }

    private fun setOpen(work: Work, key: Long, id: String, open: Boolean) = work.local(key) { local ->
        if (open) {
            local.copy(opened = local.opened - id + id, closed = local.closed - id)
        } else {
            local.copy(opened = local.opened - id, closed = local.closed + id)
        }
    }

    private fun outside(work: Work, href: String, from: Long?) {
        val base = from?.let { work.state.entry(it)?.url } ?: "/"
        when (val target = work.state.origin.resolve(href, base)) {
            is Target.Outside -> work.effects += Effect.OpenOutside(target.url)
            is Target.Inside -> work.effects += Effect.OpenOutside(work.state.origin.absolute(target.url))
            Target.Refused -> Unit
        }
    }

    // Going places.

    /** Follows `href` as found on the entry `from`. Returns the entry now in front of that stack. */
    private fun navigate(work: Work, from: Long?, href: String, present: Present?): Long? {
        val base = from?.let { work.state.entry(it)?.url } ?: "/"
        return when (val target = work.state.origin.resolve(href, base)) {
            Target.Refused -> from
            is Target.Outside -> {
                work.effects += Effect.OpenOutside(target.url)
                from
            }
            is Target.Inside -> go(work, from, target.url, present)
        }
    }

    /** Closes the sheets and dialogs of a screen that is being left. */
    private fun closeOverlays(work: Work, key: Long) = work.update(key) { entry ->
        val arrivedOpen = entry.screen?.overlays()?.mapNotNull { it.id }.orEmpty()
        entry.copy(local = entry.local.copy(opened = emptyList(), closed = entry.local.closed + arrivedOpen, confirm = null))
    }

    private fun go(work: Work, from: Long?, url: String, present: Present?): Long? {
        val state = work.state
        val route = state.shell?.routes?.let { Routes.find(it, url) }
        val how = present ?: route?.present ?: Present.Push
        if (how == Present.External) {
            work.effects += Effect.OpenOutside(state.origin.absolute(url))
            return from
        }
        if (from != null) closeOverlays(work, from)

        // A link to a top-level destination selects it instead of stacking a copy of it.
        val tab = state.tabs.indexOfFirst { Routes.path(rootUrl(state, it)) == Routes.path(url) }
        if (tab >= 0 && (how == Present.Push || how == Present.Root)) {
            val root = work.state.tabs[tab].stack.firstOrNull()
            val stack = if (root != null && root.url == url) listOf(root) else listOf(open(work, url))
            work.state = work.state.copy(
                modals = emptyList(),
                selected = tab,
                tabs = work.state.tabs.mapIndexed { index, it -> if (index == tab) it.copy(stack = stack) else it },
            )
            ensureFront(work)
            return work.state.front?.key
        }

        val anchor = from?.takeIf { work.state.entry(it) != null } ?: work.state.front?.key
        return when (how) {
            Present.Replace -> {
                val key = anchor ?: return null
                val cached = work.state.cache[url]
                work.state = work.state.copy(
                    pending = work.state.pending.filterValues { !(it is Pending.Page && it.entry == key) },
                )
                work.update(key) { old ->
                    old.copy(
                        url = url,
                        content = cached?.let { Content.Native(it) } ?: old.content,
                        loading = true,
                        failure = null,
                        stale = false,
                        local = Local(),
                    )
                }
                work.fetch(Pending.Page(key), HttpRequest("GET", url))
                key
            }
            Present.Root -> {
                val entry = open(work, url)
                work.state = work.state.copy(
                    modals = emptyList(),
                    tabs = work.state.tabs.mapIndexed { index, it ->
                        if (index == work.state.selected) it.copy(stack = listOf(entry)) else it
                    },
                )
                entry.key
            }
            Present.Sheet, Present.Full -> {
                val entry = open(work, url)
                work.state = work.state.copy(modals = work.state.modals + Modal(how, listOf(entry)))
                entry.key
            }
            else -> {
                val entry = open(work, url)
                if (anchor != null) {
                    work.stack(anchor) { it + entry }
                } else {
                    work.state = work.state.copy(
                        tabs = work.state.tabs.mapIndexed { index, it ->
                            if (index == work.state.selected) it.copy(stack = it.stack + entry) else it
                        },
                    )
                }
                entry.key
            }
        }
    }

    private fun choose(work: Work, index: Int) {
        val state = work.state
        if (state.modals.isNotEmpty() || index !in state.tabs.indices) return
        work.state = if (index == state.selected) {
            // The destination in front, chosen again: back to its first screen.
            state.copy(tabs = state.tabs.mapIndexed { at, tab -> if (at == index) tab.copy(stack = tab.stack.take(1)) else tab })
        } else {
            state.copy(selected = index)
        }
        ensureFront(work)
    }

    private fun back(work: Work) {
        val state = work.state
        val front = state.front
        if (front != null) {
            if (front.local.confirm != null) {
                work.local(front.key) { it.copy(confirm = null) }
                return
            }
            val open = openOverlay(front)
            if (open != null) {
                setOpen(work, front.key, open, false)
                return
            }
        }
        val modal = state.modals.lastOrNull()
        val tab = state.tabs.getOrNull(state.selected)
        when {
            modal != null && modal.stack.size > 1 ->
                work.state = state.copy(modals = state.modals.dropLast(1) + modal.copy(stack = modal.stack.dropLast(1)))
            modal != null -> work.state = state.copy(modals = state.modals.dropLast(1))
            tab != null && tab.stack.size > 1 ->
                work.state = state.copy(
                    tabs = state.tabs.mapIndexed { index, it -> if (index == state.selected) it.copy(stack = it.stack.dropLast(1)) else it },
                )
            // From any other destination, back leads to the first one before it leaves the app.
            state.selected != 0 -> work.state = state.copy(selected = 0)
            else -> {
                work.effects += Effect.Leave
                return
            }
        }
        ensureFront(work)
    }

    /** The sheet, dialog or popover of the entry that was opened last and is still open. */
    private fun openOverlay(entry: Entry): String? {
        val open = entry.screen?.overlays()?.filter { entry.local.isOpen(it) }?.mapNotNull { it.id }.orEmpty()
        return entry.local.opened.lastOrNull { it in open } ?: open.lastOrNull()
    }

    private fun dismiss(work: Work, key: Long) {
        val entry = work.state.entry(key) ?: return
        val open = openOverlay(entry)
        if (open != null) {
            setOpen(work, key, open, false)
        } else if (work.state.isModal(key)) {
            work.state = work.state.copy(modals = work.state.modals.filterNot { it.holds(key) })
            ensureFront(work)
        }
    }

    // Actions.

    private fun run(work: Work, key: Long, action: Action, source: String?) {
        when (action) {
            is Navigate -> navigate(work, key, action.href, action.present)
            is Submit -> submit(work, key, action)
            is Request -> request(work, key, action, source)
            is Commands -> commands(work, key, action.list)
            Back -> back(work)
            Refresh -> refetch(work, key, byUser = true)
            Dismiss -> dismiss(work, key)
            // An action this client does not know does nothing.
            is UnknownAction -> Unit
        }
    }

    private fun press(work: Work, event: Event.Pressed) {
        val button = event.button
        val entry = work.state.entry(event.entry) ?: return
        val id = button.id
        if (button.disabled || button.pending) return
        if (id != null && (id in entry.local.busy || entry.local.disabled[id] == true)) return
        val action = button.action ?: return
        // A destructive act that reaches the server asks first. One that only
        // opens the server's own dialog, or leads elsewhere, has its question there.
        val reachesServer = (action is Request && !action.method.equals("get", ignoreCase = true)) || action is Submit
        val asks = button.role == ActionRole.Destructive && reachesServer
        if (asks && !event.confirmed) {
            work.local(event.entry) { it.copy(confirm = PendingConfirm(button)) }
            return
        }
        run(work, event.entry, action, id)
    }

    private fun confirm(work: Work, event: Event.ConfirmAnswered) {
        val entry = work.state.entry(event.entry) ?: return
        val waiting = entry.local.confirm ?: return
        work.local(event.entry) { it.copy(confirm = null) }
        val action = waiting.button.action ?: return
        if (event.accepted) run(work, event.entry, action, waiting.button.id)
    }

    private fun boundary(work: Work): String = "----NavreeFormBoundary7MA4YWxkTrZu0gW${work.next()}"

    /**
     * Sends a form. A `get` asks and the answer decides: a screen takes this
     * screen's place, ops are applied to it. Anything else is a post.
     */
    private fun submit(work: Work, key: Long, action: Submit) {
        val entry = work.state.entry(key) ?: return
        val formKey = action.form ?: return
        val form = entry.screen?.form(formKey) ?: return
        if (formKey in entry.local.busy) return
        val parts = Forms.collect(form, entry.local.values[formKey].orEmpty(), entry.local.hiddenValues, action.with)
        val address = action.href ?: form.action.ifEmpty { entry.url }
        val reads = form.method.equals("get", ignoreCase = true)
        val href = if (reads) Forms.query(address, parts) else address
        val target = work.state.origin.resolve(href, entry.url) as? Target.Inside ?: return
        work.update(key) { it.copy(loading = reads, local = it.local.copy(busy = it.local.busy + formKey)) }
        work.fetch(
            Pending.Send(key, busy = formKey, write = !reads, inPlace = reads),
            HttpRequest(if (reads) "GET" else form.method.uppercase(), target.url, if (reads) null else Forms.payload(parts, boundary(work))),
        )
    }

    private fun request(work: Work, key: Long, action: Request, source: String?) {
        val entry = work.state.entry(key) ?: return
        when (val target = work.state.origin.resolve(action.href, entry.url)) {
            Target.Refused -> return
            is Target.Outside -> work.effects += Effect.OpenOutside(target.url)
            is Target.Inside -> {
                val method = action.method.uppercase()
                val reads = method == "GET"
                if (source != null) work.local(key) { it.copy(busy = it.busy + source) }
                closeOverlays(work, key)
                work.fetch(
                    Pending.Send(key, busy = source, write = !reads, inPlace = false),
                    HttpRequest(method, target.url, if (reads) null else Forms.payload(action.fields)),
                )
            }
        }
    }

    private fun commands(work: Work, key: Long, commands: List<Command>) {
        for (command in commands) {
            when (command) {
                is Command.Toggle -> setHidden(work, key, command.id, !isHidden(work.state.entry(key), command.id))
                is Command.Show -> setHidden(work, key, command.id, false)
                is Command.Hide -> setHidden(work, key, command.id, true)
                is Command.Open -> setOpen(work, key, command.id, true)
                is Command.Close -> setOpen(work, key, command.id, false)
                is Command.Copy -> work.effects += Effect.Copy(command.text)
                is Command.Set -> setState(work, key, command.id, command.state, command.on)
                is Command.Flip -> setState(work, key, command.id, command.state, !hasState(work.state.entry(key), command.id, command.state))
                is Command.Focus -> {
                    val serial = work.next()
                    work.local(key) { it.copy(focus = FocusRequest(command.id, serial)) }
                }
                is Command.SubmitForm -> submit(work, key, Submit(form = command.id))
                is Command.Text -> work.local(key) { it.copy(texts = it.texts + (command.id to command.text)) }
                is Command.Value -> work.local(key) { it.copy(hiddenValues = it.hiddenValues + (command.id to command.value)) }
                is Command.Toast -> work.say(Message(id = work.next(), tone = command.tone, text = command.text, entry = key))
                // A count belongs to typing in a field, and the field shows its own counter.
                is Command.Count, is Command.Unknown -> Unit
            }
        }
    }

    /** Whether the node is hidden now: what a command last said, or what the tree says. */
    private fun isHidden(entry: Entry?, id: String): Boolean =
        entry?.local?.hidden?.get(id) ?: ((entry?.screen?.find(id) as? StackNode)?.hidden ?: false)

    private fun setHidden(work: Work, key: Long, id: String, hidden: Boolean) =
        work.local(key) { it.copy(hidden = it.hidden + (id to hidden)) }

    private fun hasState(entry: Entry?, id: String, state: String): Boolean = when (state) {
        "hidden" -> isHidden(entry, id)
        "open" -> {
            val node = entry?.screen?.overlay(id)
            node != null && entry.local.isOpen(node)
        }
        "disabled" -> entry?.local?.disabled?.get(id) == true
        else -> entry?.local?.states?.get(id)?.contains(state) == true
    }

    /** Sets a named state. `hidden`, `open` and `disabled` change what is drawn; the rest are kept for the parts that read them. */
    private fun setState(work: Work, key: Long, id: String, state: String, on: Boolean) {
        when (state) {
            "hidden" -> setHidden(work, key, id, on)
            "open" -> setOpen(work, key, id, on)
            "disabled" -> work.local(key) { it.copy(disabled = it.disabled + (id to on)) }
            else -> work.local(key) { local ->
                val old = local.states[id].orEmpty()
                local.copy(states = local.states + (id to if (on) old + state else old - state))
            }
        }
    }

    // Search, the next page, messages, links from outside.

    private fun search(work: Work, key: Long) {
        val entry = work.state.entry(key) ?: return
        val search = entry.screen?.search ?: return
        val query = entry.local.search ?: return
        val action = search.action ?: Routes.path(entry.url)
        val with = search.hidden.map { Part.Text(it.name, it.value ?: "") }
        val parts = with + if (query.isEmpty()) emptyList() else listOf(Part.Text(search.name, query))
        val target = work.state.origin.resolve(Forms.query(action, parts), entry.url) as? Target.Inside ?: return
        if (target.url == entry.url) return
        work.update(key) { it.copy(url = target.url) }
        refetch(work, key)
    }

    private fun more(work: Work, event: Event.EndReached) {
        val entry = work.state.entry(event.entry) ?: return
        if (event.list in entry.local.loadingMore) return
        val more = moreOf(entry.screen, event.list) ?: return
        val target = work.state.origin.resolve(more, entry.url) as? Target.Inside ?: return
        work.local(event.entry) { it.copy(loadingMore = it.loadingMore + event.list) }
        work.fetch(Pending.More(event.entry, event.list, more), HttpRequest("GET", target.url))
    }

    /** The table a paging id means: the table itself, or the one in the region of that id. */
    private fun tableOf(node: Node?): TableNode? = when (node) {
        is TableNode -> node
        is RegionNode -> node.children.firstOrNull { it is TableNode } as? TableNode
        else -> null
    }

    private fun moreOf(screen: Screen?, list: String): String? = when (val node = screen?.find(list)) {
        is ListNode -> node.more
        else -> tableOf(node)?.more
    }

    private fun moreAnswered(work: Work, pending: Pending.More, response: HttpResponse) {
        val key = pending.entry
        work.local(key) { it.copy(loadingMore = it.loadingMore - pending.list) }
        when (val reply = read(response)) {
            is Reply.Failed -> {
                work.say(Message(id = work.next(), notice = reply.failure, entry = key))
                return
            }
            is Reply.Tree -> {
                val envelope = reply.envelope
                noteBuild(work, envelope.build)
                if (envelope.ops.isNotEmpty()) {
                    applyOps(work, key, envelope.ops, afterSend = false)
                } else {
                    // The next page came as a whole screen: the list or table of the same name in it is the page.
                    val entry = work.state.entry(key)
                    val screen = entry?.screen
                    val page = envelope.screen?.find(pending.list)
                    val table = tableOf(page)
                    if (entry != null && screen != null) {
                        if (page is ListNode) {
                            changeTree(work, key, Op.Append(pending.list, page.children, page.more))
                        } else if (table != null) {
                            val next = TreeOps.addPage(screen, pending.list, table)
                            work.update(key) { it.copy(content = Content.Native(next)) }
                            work.state = work.state.copy(cache = work.state.cache.put(entry.url, next))
                        }
                    }
                }
            }
            else -> Unit
        }
        // An answer that left `more` as it was has no page after it; asking again would never end.
        val entry = work.state.entry(key) ?: return
        val screen = entry.screen ?: return
        if (moreOf(screen, pending.list) == pending.was) {
            val ended = TreeOps.rewrite(screen, pending.list) { node ->
                listOf(
                    when (node) {
                        is ListNode -> node.copy(more = null)
                        is TableNode -> node.copy(more = null)
                        is RegionNode -> node.copy(children = node.children.map { if (it is TableNode) it.copy(more = null) else it })
                        else -> node
                    },
                )
            }
            work.update(key) { it.copy(content = Content.Native(ended)) }
        }
    }

    private fun messageActed(work: Work, id: Long) {
        val message = work.state.messages.firstOrNull { it.id == id } ?: return
        work.state = work.state.copy(messages = work.state.messages.filterNot { it.id == id })
        val action = message.action ?: return
        val from = message.entry?.takeIf { work.state.entry(it) != null } ?: work.state.front?.key
        if (!action.post) {
            navigate(work, from, action.href, null)
            return
        }
        val key = from ?: return
        val base = work.state.entry(key)?.url ?: "/"
        val target = work.state.origin.resolve(action.href, base) as? Target.Inside ?: return
        work.fetch(
            Pending.Send(key, busy = null, write = true, inPlace = false),
            HttpRequest("POST", target.url, Forms.payload(emptyList())),
        )
    }

    private fun follow(work: Work, event: Event.LinkFollowed) {
        val entry = work.state.entry(event.entry) ?: return
        val target = work.state.origin.resolve(event.url, entry.url)
        // The page the web view already shows is not a place to go.
        if (target is Target.Inside && target.url == entry.url) return
        navigate(work, event.entry, event.url, null)
    }

    /** An address from outside the app opens only when the shell knows it: a destination or a route. */
    private fun opened(work: Work, address: String) {
        val state = work.state
        // The one address outside the origin that opens: where the server said the newer app is.
        if (state.upgrade?.url == address && state.origin.resolve(address) is Target.Outside) {
            work.effects += Effect.OpenOutside(address)
            return
        }
        val target = state.origin.resolve(address) as? Target.Inside ?: return
        if (state.shell == null) {
            work.state = state.copy(launch = target.url)
            return
        }
        val destination = state.tabs.any { Routes.path(rootUrl(state, it)) == Routes.path(target.url) }
        val routed = Routes.find(state.shell.routes, target.url) != null
        if (destination || routed) go(work, null, target.url, null)
    }
}

import Foundation

/// How a web page's own `<input type="file">` is handled in the web view:
/// checked through ``PhotoPlace`` and ``FilmPlace`` where the system gives the
/// app ``WKUIDelegate``'s open-panel hook, and switched off with explanatory
/// words elsewhere.
public enum WebFileRules {
    /// Whether the running system lets ``WKUIDelegate`` answer a web page's
    /// file field with the app's own picker: macOS 10.12 and later, and iOS
    /// and iPadOS 18.4 and later (including iOS 26, iPadOS 26 and macOS 26).
    public static var systemAnswersOpenPanel: Bool {
        if #available(iOS 18.4, macOS 10.12, *) {
            return true
        }
        return false
    }

    /// What is shown beside a switched-off web file field where the system
    /// gives the app no hook to check picked files.
    public static var disabledWords: String {
        String(localized: "File uploads aren't available on this web page here. Use the app's own form to attach files.")
    }

    /// Whether the app may present a file picker for `frameURL`. Directory
    /// selection (`webkitdirectory`) is never opened.
    public static func allowsPanel(
        frameURL: URL?, mainFrame: Bool, site: Origin?, hosts: [String], directoryRequested: Bool
    ) -> Bool {
        if directoryRequested { return false }
        guard let frameURL else { return false }
        if mainFrame { return site?.inside(frameURL.absoluteString) != nil }
        return WebRules.allowsFrame(frameURL, site: site, hosts: hosts)
    }
}

/// What a page in the web view may load inside it. The page itself is always
/// the origin's: the web view's navigation rule sees to that. What a page
/// draws inside it (pictures, styles, scripts, fonts, frames, its own
/// requests) comes from the origin too, and from the hosts the app names for
/// the web view (`ShellConfiguration.webHosts`), which are never used for
/// anything else: not for a native request, a picture or a cookie.
///
/// Two layers say it, because neither sees everything. A list of content
/// rules, which the web engine applies to every load a page makes; and a
/// script that runs at the start of every document in every frame, before
/// the page's own, and empties a document that is not of an allowed origin
/// (a frame's address that redirected elsewhere, a page posted elsewhere).
/// The page's own Content-Security-Policy, which the framework's pages carry,
/// is the third, and the strongest.
public enum WebRules {
    /// The hosts of `hosts` that can be used: lower case, each a name of two
    /// labels or more, or `*.` and a name of two labels or more (one label or
    /// more under it). Anything else is left out.
    public static func hosts(_ hosts: [String]) -> [String] {
        hosts.map { $0.lowercased() }.filter { host in
            let name = host.hasPrefix("*.") ? String(host.dropFirst(2)) : host
            let labels = name.split(separator: ".", omittingEmptySubsequences: false)
            return labels.count >= 2
                && labels.allSatisfy { label in
                    !label.isEmpty && !label.hasPrefix("-") && !label.hasSuffix("-")
                        && label.utf8.allSatisfy { Origin.isNameByte($0) && $0 != 0x2E }
                }
        }
    }

    /// Whether `host` is one of `hosts`, a `*.` pattern standing for one
    /// label or more under its name.
    public static func covers(_ hosts: [String], _ host: String) -> Bool {
        let host = host.lowercased()
        return Self.hosts(hosts).contains { pattern in
            pattern.hasPrefix("*.") ? host.hasSuffix(String(pattern.dropFirst())) && host.count > pattern.count - 1 : host == pattern
        }
    }

    /// Whether a frame inside a page may load `url`: the origin's own, an
    /// empty frame, or a whole `https` address on the scheme's own port of a
    /// host the app names for the web view.
    public static func allowsFrame(_ url: URL, site: Origin?, hosts: [String]) -> Bool {
        if url.scheme?.lowercased() == "about" { return true }
        if site?.inside(url.absoluteString) != nil { return true }
        guard let read = Origin.read(url.absoluteString), read.scheme == "https", read.port == nil || read.port == 443
        else { return false }
        return covers(hosts, read.host)
    }

    /// The content rules, as the web engine reads them: block every load,
    /// then let through the origin's, those of the hosts the app names, and
    /// what a page makes of itself (`data:`, `blob:`, `about:`).
    public static func contentRules(site: Origin, hosts: [String]) -> String {
        var allowed = ["^" + escaped(site.serialized) + "/", "^data:", "^blob:", "^about:"]
        for host in Self.hosts(hosts) {
            if host.hasPrefix("*.") {
                allowed.append("^https://([a-z0-9-]+\\.)+" + escaped(String(host.dropFirst(2))) + "/")
            } else {
                allowed.append("^https://" + escaped(host) + "/")
            }
        }
        var rules: [[String: Any]] = [["trigger": ["url-filter": ".*"], "action": ["type": "block"]]]
        for filter in allowed {
            rules.append(["trigger": ["url-filter": filter], "action": ["type": "ignore-previous-rules"]])
        }
        let data = (try? JSONSerialization.data(withJSONObject: rules, options: [.sortedKeys, .withoutEscapingSlashes])) ?? Data("[]".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    /// The script that runs first in every document of every frame, in an
    /// isolated world: a document of another origin is stopped and emptied;
    /// file drops onto `<input type="file">` and untrusted synthetic link
    /// clicks are stopped; and when `answersFilePicker` is `false`, file
    /// fields are switched off and explain why.
    public static func documentScript(
        site: Origin, hosts: [String], answersFilePicker: Bool = WebFileRules.systemAnswersOpenPanel
    ) -> String {
        let origins = json([site.serialized])
        let named = json(Self.hosts(hosts))
        let noteWords = jsonString(WebFileRules.disabledWords)
        let answersLiteral = answersFilePicker ? "true" : "false"
        return """
            (function () {
              var origins = \(origins);
              var hosts = \(named);
              var answersFilePicker = \(answersLiteral);
              var fileNoteWords = \(noteWords);
              function named(origin) {
                var m = /^https:\\/\\/([a-z0-9.-]+)$/.exec(origin);
                if (!m) return false;
                var h = m[1];
                for (var i = 0; i < hosts.length; i++) {
                  var n = hosts[i];
                  if (n === h) return true;
                  if (n.slice(0, 2) === '*.' && h.length > n.length - 1 && h.slice(1 - n.length) === n.slice(1)) return true;
                }
                return false;
              }
              var origin = String(location.origin);
              var top = false;
              try { top = window.top === window; } catch (e) {}
              var allowed = origins.indexOf(origin) >= 0 || (!top && named(origin));
              if (!allowed) {
                try { window.stop(); } catch (e) {}
                var root = document.documentElement;
                if (root) root.remove();
                return;
              }
              function isFileInput(node) {
                return Boolean(node && node.tagName && String(node.tagName).toUpperCase() === 'INPUT'
                  && String(node.type || (node.getAttribute && node.getAttribute('type')) || '').toLowerCase() === 'file');
              }
              function linkTarget(node) {
                var cur = node;
                while (cur && cur.tagName) {
                  var tag = String(cur.tagName).toUpperCase();
                  if ((tag === 'A' || tag === 'AREA') && ((cur.hasAttribute && cur.hasAttribute('href')) || cur.href)) {
                    return cur;
                  }
                  cur = cur.parentNode;
                }
                return null;
              }
              window.addEventListener('drop', function (event) {
                var cur = event.target;
                while (cur) {
                  if (isFileInput(cur)) {
                    event.preventDefault();
                    event.stopImmediatePropagation();
                    return;
                  }
                  cur = cur.parentNode;
                }
              }, true);
              window.addEventListener('click', function (event) {
                if (event && event.isTrusted === false && !event.bubbles && linkTarget(event.target)) {
                  event.preventDefault();
                }
                if (!answersFilePicker && event) {
                  var cur = event.target;
                  while (cur) {
                    if (isFileInput(cur)) {
                      cur.disabled = true;
                      event.preventDefault();
                      event.stopImmediatePropagation();
                      return;
                    }
                    cur = cur.parentNode;
                  }
                }
              }, true);
              window.addEventListener('click', function (event) {
                if (event && event.isTrusted === false && linkTarget(event.target)) {
                  event.preventDefault();
                }
              }, false);
              if (!answersFilePicker) {
                var noted = typeof WeakSet === 'function' ? new WeakSet() : null;
                function switchOff(input) {
                  if (!isFileInput(input)) return;
                  if (!input.disabled) input.disabled = true;
                  if (input.setAttribute) input.setAttribute('aria-disabled', 'true');
                  var seen = noted ? noted.has(input) : Boolean(input.__navreeNoted);
                  if (!seen && input.parentNode && document.createElement) {
                    if (noted) noted.add(input); else input.__navreeNoted = true;
                    var note = document.createElement('p');
                    if (note.setAttribute) {
                      note.setAttribute('role', 'status');
                      note.setAttribute('data-navree-file-note', 'true');
                    }
                    note.textContent = fileNoteWords;
                    input.parentNode.insertBefore(note, input.nextSibling || null);
                  }
                }
                function sweep(scope) {
                  if (!scope) return;
                  if (isFileInput(scope)) switchOff(scope);
                  if (scope.querySelectorAll) {
                    var list = scope.querySelectorAll('input[type="file"], input');
                    for (var i = 0; i < list.length; i++) switchOff(list[i]);
                  }
                }
                sweep(document.documentElement || document);
                window.addEventListener('DOMContentLoaded', function () { sweep(document.documentElement || document); }, true);
                if (typeof MutationObserver === 'function' && (document.documentElement || document)) {
                  var observer = new MutationObserver(function (records) {
                    for (var i = 0; i < records.length; i++) {
                      var r = records[i];
                      if (r.target) switchOff(r.target);
                      if (r.addedNodes) {
                        for (var j = 0; j < r.addedNodes.length; j++) sweep(r.addedNodes[j]);
                      }
                    }
                  });
                  observer.observe(document.documentElement || document, {
                    childList: true, subtree: true, attributes: true, attributeFilter: ['type', 'disabled']
                  });
                }
              }
            })();
            """
    }

    /// The script that runs first in every document in the page's own world
    /// so a script's call to `anchor.click()` or `dispatchEvent` on a link
    /// (including a detached `<a>` never added to the document) is never
    /// turned into a user link-activation event.
    public static func pageClickScript(site: Origin) -> String {
        let allowedOrigin = jsonString(site.serialized)
        return """
            (function () {
              var allowedOrigin = \(allowedOrigin);
              function linkHref(node) {
                if (!node || !node.tagName) return null;
                var tag = String(node.tagName).toUpperCase();
                if (tag !== 'A' && tag !== 'AREA') return null;
                if (node.hasAttribute && node.hasAttribute('href')) return String(node.getAttribute('href') || '');
                if (typeof node.href === 'string' && node.href.length > 0) return node.href;
                return null;
              }
              function navigateSameOrigin(node, rawHref) {
                var target = String((node.getAttribute && node.getAttribute('target')) || node.target || '').toLowerCase();
                if (target && target !== '_self') return;
                if (node.hasAttribute && node.hasAttribute('download')) return;
                try {
                  var base = (typeof document !== 'undefined' && document.baseURI) || String(location.href);
                  var resolved = new URL(rawHref, base);
                  if (resolved.origin === allowedOrigin && location && typeof location.assign === 'function') {
                    location.assign(resolved.href);
                  }
                } catch (e) {}
              }
              var origDispatch = (typeof EventTarget !== 'undefined' && EventTarget.prototype && EventTarget.prototype.dispatchEvent)
                || function () { return true; };
              if (typeof HTMLElement !== 'undefined' && HTMLElement.prototype && typeof HTMLElement.prototype.click === 'function') {
                var origClick = HTMLElement.prototype.click;
                HTMLElement.prototype.click = function () {
                  var rawHref = linkHref(this);
                  if (rawHref === null) return origClick.apply(this, arguments);
                  navigateSameOrigin(this, rawHref);
                };
              }
              if (typeof EventTarget !== 'undefined' && EventTarget.prototype && typeof EventTarget.prototype.dispatchEvent === 'function') {
                EventTarget.prototype.dispatchEvent = function (event) {
                  if (event && String(event.type).toLowerCase() === 'click' && event.isTrusted === false && linkHref(this) !== null) {
                    if (typeof event.preventDefault === 'function') event.preventDefault();
                    return origDispatch.call(this, event);
                  }
                  return origDispatch.apply(this, arguments);
                };
              }
            })();
            """
    }

    /// `text` with every character a content rule's pattern gives a meaning escaped.
    static func escaped(_ text: String) -> String {
        var out = ""
        for character in text {
            if ".[]()+*?^$|{}\\".contains(character) { out.append("\\") }
            out.append(character)
        }
        return out
    }

    static func json(_ list: [String]) -> String {
        let data = (try? JSONSerialization.data(withJSONObject: list, options: [.withoutEscapingSlashes])) ?? Data("[]".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    static func jsonString(_ text: String) -> String {
        let array = json([text])
        guard array.count >= 2 else { return "\"\"" }
        return String(array.dropFirst().dropLast())
    }
}

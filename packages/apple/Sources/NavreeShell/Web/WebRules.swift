import Foundation

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

    /// The script that runs first in every document of every frame: a
    /// document of another origin is stopped and emptied. The page at the
    /// top must be the origin's; a frame may also be of a host the app names.
    public static func documentScript(site: Origin, hosts: [String]) -> String {
        let origins = json([site.serialized])
        let named = json(Self.hosts(hosts))
        return """
            (function () {
              var origins = \(origins);
              var hosts = \(named);
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
              if (origins.indexOf(origin) >= 0) return;
              if (!top && named(origin)) return;
              try { window.stop(); } catch (e) {}
              var root = document.documentElement;
              if (root) root.remove();
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
}

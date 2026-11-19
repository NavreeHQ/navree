using System.Text.Json;

namespace Navree.Platform;

/// <summary>What the client answers a request of the web view.</summary>
public enum WebAnswer
{
    /// <summary>It goes as it is: the web view makes it itself (a blank document, a blob, inline data).</summary>
    AsItIs,

    /// <summary>It goes to the app's own origin (or is answered from what the app recorded).</summary>
    Ours,

    /// <summary>It is answered 403 and never reaches the network.</summary>
    Refused,
}

/// <summary>
/// The folder the web view keeps what a web page leaves (cookies, storage, cache, history) and how it is taken away when no web
/// view is running to do it: the web view cannot be asked to clear what it keeps when it is not there, and a folder it has left
/// can be deleted once nothing holds its files.
/// </summary>
public static class WebData
{
    /// <summary>
    /// Why file fields and file drops are turned off in web pages on Windows: WebView2 offers no callback by which the client could
    /// inspect or strip location metadata from a chosen file before the page reads it.
    /// </summary>
    public const string FilePickerOffReason =
        "Choosing or dropping files is turned off in web pages on Windows because the system web view gives the app no way to check a file for location data before the page reads it. Use a native screen of the app to send files.";

    /// <summary>The folder the app's web views keep their data in, unless the app says another.</summary>
    /// <param name="appName">The app's name.</param>
    /// <returns>The folder's path.</returns>
    public static string DefaultFolder(string appName) =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Navree", appName, "web");

    /// <summary>Deletes the folder and everything in it, trying again while a file is still held by a process that is leaving.</summary>
    /// <param name="folder">The folder.</param>
    /// <param name="limit">How long to keep trying.</param>
    /// <param name="pause">How long between tries; a tenth of a second when left out.</param>
    /// <returns>Whether the folder is gone.</returns>
    public static async Task<bool> DeleteAsync(string folder, TimeSpan limit, TimeSpan? pause = null)
    {
        ArgumentException.ThrowIfNullOrEmpty(folder);
        TimeSpan between = pause ?? TimeSpan.FromMilliseconds(100);
        DateTime until = DateTime.UtcNow + limit;
        while (true)
        {
            try
            {
                if (!Directory.Exists(folder))
                {
                    return true;
                }

                Directory.Delete(folder, recursive: true);
                return !Directory.Exists(folder);
            }
            catch (Exception failure) when (failure is IOException or UnauthorizedAccessException)
            {
                if (DateTime.UtcNow >= until)
                {
                    return false;
                }
            }

            await Task.Delay(between).ConfigureAwait(false);
        }
    }

    /// <summary>Whether a web address is one the web view makes itself and not a page: a blank document or one made from the page's own data.</summary>
    /// <param name="address">The address.</param>
    /// <returns>True for a blank page and a blob.</returns>
    public static bool IsOwnDocument(string address) =>
        address.StartsWith("about:", StringComparison.OrdinalIgnoreCase)
        || address.StartsWith("blob:", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// What the client answers a request the web view asks it about, whichever part of a page made it (the page, a frame, a worker of any
    /// kind): what the web view makes itself goes as it is; an address that is not the app's own is refused; and while a sign-out's steps run
    /// or the web view's screen is covered, every request that would reach the network is refused.
    /// </summary>
    /// <param name="address">The whole address asked for.</param>
    /// <param name="ours">Whether an address is one of the app's own.</param>
    /// <param name="wiping">Whether a sign-out's steps are running.</param>
    /// <param name="covered">Whether the web view's screen is covered or closed.</param>
    /// <returns>What to do with it.</returns>
    public static WebAnswer Answer(string address, Func<string, bool> ours, bool wiping, bool covered = false)
    {
        ArgumentNullException.ThrowIfNull(address);
        ArgumentNullException.ThrowIfNull(ours);
        if (IsOwnRequest(address))
        {
            return WebAnswer.AsItIs;
        }

        return !wiping && !covered && ours(address) ? WebAnswer.Ours : WebAnswer.Refused;
    }

    /// <summary>
    /// Whether an address a web page was sent to is a file of this computer: what the web view makes of a file dropped on a page that does not
    /// take drops. Such an address holds a path of the person's own (their Windows user name, a folder's name), so it is cancelled where it
    /// arrives and told to nobody: not to the core, and not to a log.
    /// </summary>
    /// <param name="address">The address.</param>
    /// <returns>True for an address on the <c>file</c> scheme, and for a path written without one.</returns>
    public static bool IsLocalFile(string? address)
    {
        if (string.IsNullOrWhiteSpace(address))
        {
            return false;
        }

        string text = address.TrimStart();
        if (text.StartsWith("file:", StringComparison.OrdinalIgnoreCase) || text.StartsWith(@"\\", StringComparison.Ordinal))
        {
            return true;
        }

        // A drive's path: a letter, a colon, a slash.
        return text.Length >= 3 && char.IsAsciiLetter(text[0]) && text[1] == ':' && text[2] is '\\' or '/';
    }

    /// <summary>Whether a request is one the web view answers without the network: a document of its own, or inline data of a page.</summary>
    /// <param name="address">The address.</param>
    /// <returns>True for a blank page, a blob and inline data.</returns>
    public static bool IsOwnRequest(string address) =>
        IsOwnDocument(address) || address.StartsWith("data:", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Builds the script run on every document created in the web view before anything in the page runs: empties any document whose
    /// origin is not one of <paramref name="allowedOrigins"/>, disables <c>&lt;input type="file"&gt;</c>, blocks programmatic file
    /// pickers, file drops and file pastes, and writes <see cref="FilePickerOffReason"/> into the page so the person sees why.
    /// </summary>
    /// <param name="allowedOrigins">The document origins the app allows.</param>
    /// <returns>The script source.</returns>
    public static string DocumentGuardScript(IReadOnlyList<string> allowedOrigins)
    {
        ArgumentNullException.ThrowIfNull(allowedOrigins);
        string allowed = JsonSerializer.Serialize(allowedOrigins);
        string why = JsonSerializer.Serialize(FilePickerOffReason);
        return
            "(function(){var allowed=" + allowed + ";var why=" + why + ";" +
            "try{var o=location.origin;if(o!=='null'&&location.protocol!=='about:'&&location.protocol!=='data:'){" +
            "var ok=allowed.some(function(a){var star=a.indexOf('://*.');if(star<0){return a===o;}var scheme=a.slice(0,star+3);var rest=a.slice(star+4);return o.indexOf(scheme)===0&&o.slice(scheme.length).endsWith(rest)&&o.slice(scheme.length).length>rest.length;});" +
            "if(!ok){window.stop();if(document.documentElement){document.documentElement.innerHTML='';}return;}}}catch(e){}" +
            "var typeDesc=(typeof HTMLInputElement!=='undefined'&&Object.getOwnPropertyDescriptor)?Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'type'):null;" +
            "var typeGet=typeDesc&&typeDesc.get?typeDesc.get:null;" +
            "function isFileInput(el){if(!el||el.nodeType!==1){return false;}try{if(typeGet&&String(typeGet.call(el)).toLowerCase()==='file'){return true;}}catch(e){}return String(el.tagName||'').toUpperCase()==='INPUT'&&String(el.type||'').toLowerCase()==='file';}" +
            "function labelFileControl(lbl){if(!lbl||String(lbl.tagName||'').toUpperCase()!=='LABEL'){return null;}if(isFileInput(lbl.control)){return lbl.control;}var id=lbl.getAttribute&&lbl.getAttribute('for');if(id){var rn=lbl.getRootNode?lbl.getRootNode():null;if(rn&&rn.getElementById){var hit=rn.getElementById(id);if(isFileInput(hit)){return hit;}}if(document.getElementById){var docHit=document.getElementById(id);if(isFileInput(docHit)){return docHit;}}}return lbl.querySelector?fileInSubtree(lbl):null;}" +
            "function fileInSubtree(root){if(!root||!root.querySelectorAll){return null;}var inputs=root.querySelectorAll('input');for(var i=0;i<inputs.length;i++){if(isFileInput(inputs[i])){return inputs[i];}}return null;}" +
            "function fileTargetFromNode(el){for(var cur=el;cur&&cur.nodeType===1;cur=cur.parentElement){if(isFileInput(cur)){return cur;}var via=labelFileControl(cur);if(via){return via;}}return null;}" +
            "function fileTargetFromEvent(e){var path=(e&&typeof e.composedPath==='function')?e.composedPath():(e&&e.target?[e.target]:[]);for(var i=0;i<path.length;i++){var n=path[i];if(isFileInput(n)){return n;}var via=labelFileControl(n);if(via){return via;}var anc=fileTargetFromNode(n);if(anc){return anc;}}return null;}" +
            "function tellWhy(anchor){try{var doc=(anchor&&anchor.ownerDocument)||document;var root=(anchor&&anchor.getRootNode)?anchor.getRootNode():doc;var box=(root&&root.getElementById)?root.getElementById('__navree_file_off'):doc.getElementById('__navree_file_off');if(!box){box=doc.createElement('div');box.id='__navree_file_off';box.setAttribute('role','status');box.setAttribute('aria-live','polite');box.style.cssText='margin:8px 0;padding:10px 12px;border:1px solid currentColor;border-radius:6px;font:13px/1.4 system-ui,Segoe UI,sans-serif;background:Canvas;color:CanvasText;';box.textContent=why;}if(anchor&&anchor.parentNode){anchor.parentNode.insertBefore(box,anchor.nextSibling);}else if(doc.body&&!box.parentNode){doc.body.insertBefore(box,doc.body.firstChild);}}catch(e){}}" +
            "function disableInput(el){el.disabled=true;el.setAttribute('aria-disabled','true');el.setAttribute('title',why);tellWhy(el);}" +
            "var observed=typeof WeakSet!=='undefined'?new WeakSet():null;" +
            "function markInputs(root){try{if(!root||!root.querySelectorAll){return;}var list=root.querySelectorAll('*');for(var i=0;i<list.length;i++){if(isFileInput(list[i])){disableInput(list[i]);}if(list[i].shadowRoot){observeRoot(list[i].shadowRoot);}}}catch(e){}}" +
            "function observeRoot(root){try{if(!root||(observed&&observed.has(root))){return;}if(observed){observed.add(root);}markInputs(root);new MutationObserver(function(records){for(var i=0;i<records.length;i++){var r=records[i];if(r.type==='attributes'&&isFileInput(r.target)){disableInput(r.target);}for(var j=0;j<r.addedNodes.length;j++){var n=r.addedNodes[j];if(n.nodeType===1){if(isFileInput(n)){disableInput(n);}if(n.shadowRoot){observeRoot(n.shadowRoot);}markInputs(n);}}}}).observe(root,{subtree:true,childList:true,attributes:true,attributeFilter:['type','disabled']});}catch(e){}}" +
            "try{if(typeof Element!=='undefined'&&typeof Element.prototype.attachShadow==='function'){var rawShadow=Element.prototype.attachShadow;Object.defineProperty(Element.prototype,'attachShadow',{configurable:false,writable:false,value:function(){var sr=rawShadow.apply(this,arguments);observeRoot(sr);return sr;}});}}catch(e){}" +
            "try{if(typeof HTMLElement!=='undefined'&&typeof HTMLElement.prototype.click==='function'){var rawHtmlClick=HTMLElement.prototype.click;Object.defineProperty(HTMLElement.prototype,'click',{configurable:false,writable:false,value:function(){if(isFileInput(this)){disableInput(this);return;}return rawHtmlClick.apply(this,arguments);}});}}catch(e){}" +
            "try{if(typeof HTMLInputElement!=='undefined'){var rawClick=HTMLInputElement.prototype.click;Object.defineProperty(HTMLInputElement.prototype,'click',{configurable:false,writable:false,value:function(){if(isFileInput(this)){disableInput(this);return;}return rawClick.apply(this,arguments);}});if(typeof HTMLInputElement.prototype.showPicker==='function'){var rawShow=HTMLInputElement.prototype.showPicker;Object.defineProperty(HTMLInputElement.prototype,'showPicker',{configurable:false,writable:false,value:function(){if(isFileInput(this)){disableInput(this);throw new DOMException(why,'NotAllowedError');}return rawShow.apply(this,arguments);}});}}}catch(e){}" +
            "function denyPicker(){tellWhy(null);return Promise.reject(new DOMException(why,'NotAllowedError'));}" +
            "try{['showOpenFilePicker','showDirectoryPicker','showSaveFilePicker'].forEach(function(fn){if(fn in window){Object.defineProperty(window,fn,{configurable:false,writable:false,value:denyPicker});}});}catch(e){}" +
            "window.addEventListener('click',function(e){var hit=fileTargetFromEvent(e);if(hit){e.preventDefault();e.stopImmediatePropagation();disableInput(hit);}},true);" +
            "window.addEventListener('keydown',function(e){if(e.key===' '||e.key==='Enter'){var hit=fileTargetFromEvent(e);if(hit){e.preventDefault();e.stopImmediatePropagation();disableInput(hit);}}},true);" +
            "function hasFiles(dt){if(!dt){return false;}if(dt.files&&dt.files.length>0){return true;}if(dt.items){for(var i=0;i<dt.items.length;i++){if(dt.items[i].kind==='file'){return true;}}}if(dt.types){for(var j=0;j<dt.types.length;j++){if(dt.types[j]==='Files'){return true;}}}return false;}" +
            "window.addEventListener('dragover',function(e){if(hasFiles(e.dataTransfer)){e.preventDefault();e.stopImmediatePropagation();try{e.dataTransfer.dropEffect='none';}catch(err){}}},true);" +
            "window.addEventListener('drop',function(e){if(hasFiles(e.dataTransfer)){e.preventDefault();e.stopImmediatePropagation();tellWhy(e.target&&e.target.nodeType===1?e.target:null);}},true);" +
            "window.addEventListener('paste',function(e){if(hasFiles(e.clipboardData)){e.preventDefault();e.stopImmediatePropagation();tellWhy(e.target&&e.target.nodeType===1?e.target:null);}},true);" +
            "observeRoot(document);" +
            "if(document.readyState==='loading'){document.addEventListener('DOMContentLoaded',function(){markInputs(document);},{once:true});}" +
            "})();";
    }
}

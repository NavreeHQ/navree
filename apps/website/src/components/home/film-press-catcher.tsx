import { PRESSED_EARLY, STOP_CATCHING } from "./film-press";

/**
 * Notes a press on the film's play button made before the page's scripts have
 * loaded and hydrated the player, which would otherwise do nothing: it sets a
 * flag that the player reads when it mounts and starts the film.
 *
 * It listens on the document, so it needs no element after it and can go at
 * the start of the hero. An inline script waits for the page's stylesheets,
 * and the browser may paint what it has parsed while it waits; placed after the
 * player, that paint showed the hero cut short, and the rest of it then moved
 * the hero's backdrop. The player removes the listener once it is hydrated,
 * so a later visit to the page never finds the flag set by an old press.
 */
export function FilmPressCatcher() {
  const script = `(function(w,d){function f(e){var t=e.target;if(t&&t.closest&&t.closest("[data-film-play]"))w.${PRESSED_EARLY}=true}d.addEventListener("click",f,true);w.${STOP_CATCHING}=function(){d.removeEventListener("click",f,true)}})(window,document)`;
  return (
    <script
      // A fixed string built in this file, with no input from a visitor.
      // nosemgrep: navree-raw-html
      dangerouslySetInnerHTML={{ __html: script }}
    />
  );
}

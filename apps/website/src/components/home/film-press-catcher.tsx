import { PRESSED_EARLY } from "./film-press";

/**
 * Notes a press on the film's play button made before the page's scripts have
 * loaded and hydrated the player, which would otherwise do nothing: it sets a
 * flag that the player reads when it mounts and starts the film.
 *
 * Put it right after the `FilmPlayer`, in the same parent. It runs while the
 * page is parsed, and adds one click listener to the button; it reads and
 * changes nothing else, so hydration finds the markup as the server wrote it.
 */
export function FilmPressCatcher() {
  const script = `(function(s){var p=s.parentElement,b=p&&p.querySelector("[data-film-play]");if(b)b.addEventListener("click",function(){window.${PRESSED_EARLY}=true},{once:true})})(document.currentScript)`;
  return (
    <script
      // A fixed string built in this file, with no input from a visitor.
      // nosemgrep: navree-raw-html
      dangerouslySetInnerHTML={{ __html: script }}
    />
  );
}

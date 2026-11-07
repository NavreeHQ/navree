/**
 * The flag that `FilmPressCatcher` (a server component, an inline script) sets
 * on `window` when the film's play button is pressed before the page's scripts
 * have loaded, and that `FilmPlayer` (a client component) reads when it mounts.
 * A module of its own, because a server component cannot read a constant out of
 * a client component's file.
 */
export const PRESSED_EARLY = "__navreeFilmPress";

/** The function the catcher leaves on `window` that removes its listener, called by the player once it is hydrated. */
export const STOP_CATCHING = "__navreeFilmPressStop";

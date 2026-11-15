// A component whose script asks for a file of another site. The page's policy allows no script from
// anywhere else, so this module never loads: its element keeps what the server drew in its place.
import "https://cdn.example/x.js";

export const contract = 1;

export function mount() {}

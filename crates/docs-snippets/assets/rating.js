// The rating's script. The server draws the five stars. When the rating can be changed, this turns
// them into buttons: a rating that is a form field moves the field, and one that has a route of its
// own posts the new value there.
export const contract = 1;

const MOST = 5;

export function mount(el, props, ui) {
  const body = ui.find("[data-slot='app:rating-stars']");
  const field = props.pick ? ui.find(`input[name="${CSS.escape(props.pick)}"]`) : null;
  // Only shown: the stars the server drew stay as they are.
  if (!body || (!field && !props.rate_at)) return;

  let current = props;

  // The stars as buttons, filled up to the value. A star that had the focus keeps it.
  const draw = () => {
    const focused = [...body.children].indexOf(document.activeElement);
    body.setAttribute("role", "group");
    body.setAttribute("aria-label", "Rating");
    body.replaceChildren(
      ...Array.from({ length: MOST }, (_, at) => {
        const star = document.createElement("button");
        star.type = "button";
        star.setAttribute("data-slot", "app:rating-star");
        star.setAttribute("aria-label", `${at + 1} of ${MOST}`);
        star.setAttribute("aria-pressed", String(at + 1 === current.value));
        if (at < current.value) star.setAttribute("data-selected", "");
        star.textContent = "★";
        return star;
      }),
    );
    if (focused >= 0) body.children[focused].focus();
  };

  const choose = async (value) => {
    const before = current.value;
    current = { ...current, value };
    draw();
    if (field) {
      // An inline edit posts when the field changes; a form takes the value when it is sent.
      field.value = String(value);
      field.dispatchEvent(new Event("change", { bubbles: true }));
      return;
    }
    el.setAttribute("data-busy", "");
    const answer = await ui.call(current.rate_at, { stars: String(value) });
    el.removeAttribute("data-busy");
    if (!answer.ok) {
      current = { ...current, value: before };
      draw();
    }
  };

  ui.on(body, "click", (event) => {
    const star = ui.closest(event.target, "button[data-slot='app:rating-star']");
    if (star) choose([...body.children].indexOf(star) + 1);
  });
  // Enter on a star chooses it: it does not end the edit of a table cell around it.
  ui.on(body, "keydown", (event) => {
    if (event.key === "Enter") event.stopPropagation();
  });
  // Typing in the field moves the stars.
  if (field) {
    ui.on(field, "input", () => {
      current = { ...current, value: Math.min(MOST, Math.max(0, Number(field.value) || 0)) };
      draw();
    });
  }

  draw();
  return {
    update(next) {
      current = next;
      draw();
    },
  };
}

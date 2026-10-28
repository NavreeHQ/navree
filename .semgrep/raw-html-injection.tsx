export function Bad({ html }: { html: string }) {
  // ruleid: navree-raw-html
  return <div dangerouslySetInnerHTML={{ __html: html }} />;
}

export function BadChildren({ html }: { html: string }) {
  return (
    // ruleid: navree-raw-html
    <span className="x" dangerouslySetInnerHTML={{ __html: html }}>
      ignored
    </span>
  );
}

export function Good({ text }: { text: string }) {
  // ok: navree-raw-html
  return <div>{text}</div>;
}

export function imperative(el: HTMLElement, user: string) {
  // ruleid: navree-raw-html
  el.innerHTML = user;
  // ruleid: navree-raw-html
  el.insertAdjacentHTML("beforeend", user);
  // ruleid: navree-raw-html
  document.write(user);
  // ok: navree-raw-html
  el.textContent = user;
  // ok: navree-raw-html
  el.innerHTML = "";
}

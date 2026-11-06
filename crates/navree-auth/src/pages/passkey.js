// Signs a browser in with a passkey: asks the server for the challenge, lets
// the browser and the authenticator answer it, and posts the answer back. The
// page is `AuthPages`' passkey page; every post carries the session's CSRF
// token as `X-XSRF-TOKEN`, and a refusal shows one plain message.
(() => {
  "use strict";
  const button = document.getElementById("passkey-sign-in");
  const failure = document.getElementById("passkey-error");
  if (!button || !failure) return;
  const base = button.getAttribute("data-base") || "";
  const next = button.getAttribute("data-next") || "";

  const show = (text) => {
    failure.textContent = text;
    failure.hidden = false;
  };
  const cookie = (name) => {
    for (const part of document.cookie.split(";")) {
      const at = part.indexOf("=");
      if (at > 0 && part.slice(0, at).trim() === name) {
        try {
          return decodeURIComponent(part.slice(at + 1).trim());
        } catch {
          return "";
        }
      }
    }
    return "";
  };
  const post = async (url, body) => {
    const response = await fetch(url, {
      method: "POST",
      credentials: "same-origin",
      headers: {
        "content-type": "application/json",
        accept: "application/json",
        "x-xsrf-token": cookie("XSRF-TOKEN"),
      },
      body: JSON.stringify(body),
    });
    const data = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error("refused");
    return data;
  };

  if (!window.PublicKeyCredential || !PublicKeyCredential.parseRequestOptionsFromJSON) {
    show("This browser can't use passkeys. Sign in with your password instead.");
    button.disabled = true;
    return;
  }

  button.addEventListener("click", async () => {
    failure.hidden = true;
    button.disabled = true;
    try {
      const options = await post(base + "/options", {});
      const credential = await navigator.credentials.get({
        publicKey: PublicKeyCredential.parseRequestOptionsFromJSON(options.publicKey),
      });
      const done = await post(base + "/verify", { credential: credential.toJSON(), next });
      if (typeof done.next === "string" && done.next.startsWith("/") && !done.next.startsWith("//")) {
        location.assign(done.next);
      } else {
        location.assign("/");
      }
    } catch (error) {
      show(
        error && error.name === "NotAllowedError"
          ? "The passkey was not used. Try again, or sign in with your password."
          : "That passkey didn't work. Try again, or sign in with your password.",
      );
      button.disabled = false;
    }
  });
})();

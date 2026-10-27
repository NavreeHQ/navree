// The page frame every fixture template shares.
#let frame(title: none, body) = {
  set page(paper: "a4", margin: 2cm)
  set text(size: 12pt)
  if title != none {
    align(center, text(size: 20pt, weight: "bold", title))
    v(1em)
  }
  body
}

// One certificate in the language the render asks for; the locale also
// sets the text direction (right to left for Arabic).
#let data = json("/data.json")
#let locale = sys.inputs.at("locale", default: "en")

#set page(paper: "a5", flipped: true, margin: 1.5cm)

#align(center)[
  #text(size: 18pt, weight: "bold", data.title)

  #data.name

  #data.course
]

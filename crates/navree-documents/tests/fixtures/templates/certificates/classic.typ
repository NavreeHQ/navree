#import "/shared/layout.typ": frame

#let data = json("/data.json")

#show: frame.with(title: "Certificate of completion")

This certifies that #strong(data.name) completed #emph(data.course) on #data.completed_on.

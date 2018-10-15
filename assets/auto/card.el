(TeX-add-style-hook
 "card"
 (lambda ()
   (TeX-add-to-alist 'LaTeX-provided-class-options
                     '(("article" "10pt")))
   (TeX-add-to-alist 'LaTeX-provided-package-options
                     '(("graphicx" "dvips") ("inputenc" "utf8") ("fontenc" "T1")))
   (TeX-run-style-hooks
    "latex2e"
    "article"
    "art10"
    "graphicx"
    "inputenc"
    "fontenc"
    "xcolor"
    "tikz"
    "geometry"
    "tgadventor")
   (TeX-add-symbols
    "Name"
    "Description"
    "Email"
    "Phone")
   (LaTeX-add-lengths
    "cardw"
    "cardh"))
 :latex)


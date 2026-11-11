# The website's picture

The island at the top of the website is drawn by a script, not by hand, so
it can be changed and drawn again.

| File | What it is |
| --- | --- |
| `scene.py` | Draws the scene in isometric projection and writes it as a page, one layer to an `<svg>` |
| `layers.sh` | Draws the scene and cuts it into the thirteen pictures in `site/img/scene`, and says where each goes on the stage |
| `tex.sh` | Rounds the corners of a screenshot and leans it onto a face of the scene |
| `tex-*.png` | Screenshots of the app, already leaned: the sheet and the three cards lifted off it |

To change the picture: edit `scene.py`, run `./layers.sh`, and if a layer
moved or changed size, copy its new place into `site/index.html`.

It needs Python 3, Chrome, ImageMagick and `cwebp`. The design of the whole
page is the page "Website" of `design/zinMD-UI.figcraft`.

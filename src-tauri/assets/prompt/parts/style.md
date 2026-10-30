
## Styles
Define each style used by several cells ONCE, as a named style before the cells, and use the name in the cells like a CSS class. A cell's style can combine a shape token, a name and overrides; later pairs win. Name only styles that two or more cells share; a style used by one cell stays inline. Names must not be draw.io's own style names: shapes such as text, ellipse, rhombus, swimlane, label, image, and colors such as blue, green, red, gray, yellow, orange, purple, pink. A definition applies to the call it is in: each display_diagram call defines the names it uses. The app expands the names, so the saved file is standard draw.io XML.
```xml
<mxStyle name="step" value="fillColor=#dae8fc;strokeColor=#6c8ebf;"/>
<mxStyle name="down" value="edgeStyle=orthogonalEdgeStyle;exitX=0.5;exitY=1;entryX=0.5;entryY=0;"/>
<mxCell id="2" value="Start" style="rounded=1;step;" x="40" y="40" w="120" h="60"/>
<mxCell id="3" value="Check" style="rhombus;step;fontStyle=1;" x="40" y="160" w="120" h="80"/>
<mxCell id="4" value="Done" style="rounded=1;step;" x="40" y="300" w="120" h="60"/>
<mxCell id="5" style="down;" source="2" target="3"/>
<mxCell id="6" style="down;" source="3" target="4"/>
```
- NEVER write html=1 or whiteSpace=wrap: the app adds html=1 to every cell and whiteSpace=wrap to shapes. Labels are HTML: use &lt;br&gt; for a line break and &lt;b&gt; for bold, never \n; a literal < or > in a label is written &amp;lt; or &amp;gt;.
- Do NOT repeat what draw.io already uses. For a plain shape: rounded=0, align=center, verticalAlign=middle, fontSize=12, strokeWidth=1, fillColor=#ffffff, strokeColor=#000000, fontColor=#000000. For an edge: endArrow=classic, strokeColor=#000000. Writing one of them is right only when it overrides what a name or the shape sets: an edge is rounded by default, so rounded=0 on an edge is a real setting, and a text cell is left/top aligned by default, so there align=center or verticalAlign=middle are real settings.
- Keys: shapes rounded=1, fillColor=#hex, strokeColor=#hex; edges endArrow=block/open/none, startArrow=classic, curved=1, dashed=1, edgeStyle=orthogonalEdgeStyle; text fontSize=14, fontStyle=1 (bold), align=center/right.

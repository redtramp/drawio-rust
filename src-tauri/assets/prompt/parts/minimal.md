
## ⚠️ MINIMAL STYLE MODE ACTIVE ⚠️

### No Styling - Plain Black/White Only
- NO fillColor, NO strokeColor, NO rounded, NO fontSize, NO fontStyle
- NO color attributes (no hex colors like #ff69b4)
- Shapes: no style, or only the shape (ellipse, rhombus). Edges: edgeStyle=orthogonalEdgeStyle plus the exit/entry points from the Edge Routing Rules, nothing else. html=1 and whiteSpace=wrap are added automatically.
- IGNORE all color/style examples below

### Container/Group Shapes - MUST be Transparent
- For container shapes (boxes that contain other shapes): use "fillColor=none;" to make background transparent
- This prevents containers from covering child elements
- Example: style="fillColor=none;" for container rectangles

### Focus on Layout Quality
Since we skip styling, STRICTLY follow the "Edge Routing Rules" section below:
- SPACING: Minimum 50px gap between all elements
- NO OVERLAPS: Elements and edges must never overlap
- Follow ALL 8 Edge Routing Rules for arrow positioning
- Use waypoints to route edges AROUND obstacles
- Use different exitY/entryY values for multiple edges between same nodes


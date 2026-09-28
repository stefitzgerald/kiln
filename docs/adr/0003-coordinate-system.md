# ADR 0003: Coordinate system, clip space and depth conventions

**Status:** Accepted (M0)

## Decision
| Aspect | Convention |
|---|---|
| Handedness | Right-handed |
| Up / forward / right | +Y / −Z / +X (same as glTF and Godot, so no conversion on import) |
| Units | Meters |
| Front faces | Counter-clockwise when viewed from the front |
| Clip space | Y-up, depth `[0, 1]` (DirectX/WebGPU convention) |
| Depth | **Reverse-Z**: near → 1, far → 0, `D32_SFLOAT`, clear 0, compare `GREATER_OR_EQUAL` |
| Y flip | Negative viewport height in the Vulkan backend. Neither matrices nor shaders flip. |
| Color | Linear in shaders. sRGB swapchain/offscreen formats encode on write. Texture color space comes from use (base color = sRGB). |

## Rationale
- Matching glTF and Godot conventions removes an entire class of import bugs.
- Reverse-Z with floating-point depth gives near-uniform precision across the view
  distance, which eliminates most z-fighting in large scenes.
- Flipping Y through the viewport keeps projection matrices identical to WebGPU/D3D, so a
  future non-Vulkan backend needs no math changes. Front-face winding stays CCW (the
  negative viewport preserves the conventional orientation).

## Verification
TC-MATH-02/03 check the math. TC-GPU-03 checks that the red triangle vertex is at the
top (Y-up). TC-GPU-04 checks that front faces are lit (winding and culling). TC-GPU-05
checks reverse-Z ordering. TC-GPU-02 and TC-MAN-04 check sRGB.

/**
 * Concentric pulsing circles animation.
 *
 * Color inherits from `currentColor` — set `text-*` on a parent to control color.
 * Use as a standalone loading indicator or inside `ReloadingOverlay`.
 */
export function Ripple({ size = "md" }: { size?: "sm" | "md" | "lg" }) {
  const outerPx = size === "sm" ? 32 : size === "lg" ? 96 : 64

  const circles = [
    { scale: 1.0,  opacity: 0.10, delay: "600ms" },
    { scale: 0.75, opacity: 0.20, delay: "400ms" },
    { scale: 0.50, opacity: 0.35, delay: "200ms" },
    { scale: 0.28, opacity: 0.55, delay: "0ms"   },
  ]

  return (
    <>
      <style>{`
        @keyframes ripple-pulse {
          0%, 100% { transform: translate(-50%, -50%) scale(0.4); }
          50%       { transform: translate(-50%, -50%) scale(1.0); }
        }
      `}</style>
      <div
        className="relative"
        style={{ width: outerPx, height: outerPx }}
        aria-hidden="true"
      >
        {circles.map(({ scale, opacity, delay }, i) => (
          <div
            key={i}
            className="absolute rounded-full bg-current"
            style={{
              width:  outerPx * scale,
              height: outerPx * scale,
              left: "50%",
              top:  "50%",
              opacity,
              animation: `ripple-pulse 1.6s ease-in-out ${delay} infinite`,
            }}
          />
        ))}
      </div>
    </>
  )
}

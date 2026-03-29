import { useEffect, useRef } from "react";

interface MobiusRingProps {
  size?: number;
  color?: string;
  speed?: number;
}

export function MobiusRing({
  size = 24,
  color = "#4f46e5",
  speed = 2
}: MobiusRingProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const displaySize = size;
    canvas.width = displaySize * dpr;
    canvas.height = displaySize * dpr;
    canvas.style.width = `${displaySize}px`;
    canvas.style.height = `${displaySize}px`;
    ctx.scale(dpr, dpr);

    let animationId: number;
    let time = 0;

    const draw = () => {
      ctx.clearRect(0, 0, displaySize, displaySize);

      const centerX = displaySize / 2;
      const centerY = displaySize / 2;
      const radiusX = displaySize * 0.35;
      const thickness = displaySize * 0.08;

      // Parse color to RGB for gradient
      const tempDiv = document.createElement("div");
      tempDiv.style.color = color;
      document.body.appendChild(tempDiv);
      const computedColor = getComputedStyle(tempDiv).color;
      document.body.removeChild(tempDiv);

      const rgbMatch = computedColor.match(/\d+/g);
      const r = rgbMatch ? parseInt(rgbMatch[0]) : 79;
      const g = rgbMatch ? parseInt(rgbMatch[1]) : 70;
      const b = rgbMatch ? parseInt(rgbMatch[2]) : 229;

      // Draw Mobius-like infinity/twisted ring using two overlapping ellipses
      const segments = 60;
      const points: { x: number; y: number; z: number }[] = [];

      // Generate Mobius strip points using parametric equations
      for (let i = 0; i <= segments; i++) {
        const t = (i / segments) * Math.PI * 2;
        const twist = t / 2; // Half twist for Mobius

        // Mobius strip parametric equations (simplified for 2D projection)
        const R = radiusX * 0.8;
        const w = thickness * 2;

        const x = (R + w * Math.cos(twist) * Math.cos(t)) * Math.cos(t + time * speed);
        const y = (R + w * Math.cos(twist) * Math.cos(t)) * Math.sin(t + time * speed) * 0.5;
        const z = w * Math.sin(twist) * Math.cos(t);

        points.push({ x: centerX + x, y: centerY + y, z });
      }

      // Draw the strip
      ctx.lineCap = "round";
      ctx.lineJoin = "round";

      for (let i = 0; i < points.length - 1; i++) {
        const p1 = points[i];
        const p2 = points[i + 1];

        // Depth-based opacity and thickness
        const avgZ = (p1.z + p2.z) / 2;
        const normalizedZ = (avgZ + thickness * 2) / (thickness * 4);
        const opacity = 0.4 + normalizedZ * 0.6;
        const lineWidth = thickness * (0.5 + normalizedZ * 0.5);

        ctx.beginPath();
        ctx.moveTo(p1.x, p1.y);
        ctx.lineTo(p2.x, p2.y);
        ctx.strokeStyle = `rgba(${r}, ${g}, ${b}, ${opacity})`;
        ctx.lineWidth = lineWidth;
        ctx.stroke();
      }

      // Add glow effect
      ctx.shadowColor = color;
      ctx.shadowBlur = 4;

      time += 0.016;
      animationId = requestAnimationFrame(draw);
    };

    draw();

    return () => {
      cancelAnimationFrame(animationId);
    };
  }, [size, color, speed]);

  return (
    <canvas
      ref={canvasRef}
      style={{
        width: size,
        height: size,
        filter: `drop-shadow(0 0 ${size * 0.1}px ${color})`,
      }}
    />
  );
}

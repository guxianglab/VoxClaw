import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

interface AudioVisualizerProps {
    isRecording: boolean;
    className?: string;
    eventName?: string;
}

// Minimalist bar for audio visualization
class Bar {
    x: number;
    targetHeight: number;
    height: number;
    width: number;

    constructor(x: number, width: number) {
        this.x = x;
        this.width = width;
        this.height = 0;
        this.targetHeight = 0;
    }

    update(level: number, maxHeight: number) {
        this.targetHeight = level * maxHeight * (0.3 + Math.random() * 0.7);
        this.height += (this.targetHeight - this.height) * 0.2;
    }

    draw(ctx: CanvasRenderingContext2D, centerY: number, color: string) {
        const halfHeight = this.height / 2;
        ctx.fillStyle = color;
        ctx.fillRect(this.x, centerY - halfHeight, this.width, this.height);
    }
}

export function AudioVisualizer({ isRecording, className, eventName = "audio_level" }: AudioVisualizerProps) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const audioLevelRef = useRef(0);
    const smoothedLevelRef = useRef(0);
    const animationFrameRef = useRef<number | null>(null);
    const barsRef = useRef<Bar[]>([]);

    useEffect(() => {
        // Initialize bars
        if (barsRef.current.length === 0) {
            const barCount = 24;
            const barWidth = 3;
            const gap = 4;
            const totalWidth = barCount * (barWidth + gap);
            const startX = (300 - totalWidth) / 2;

            for (let i = 0; i < barCount; i++) {
                barsRef.current.push(new Bar(startX + i * (barWidth + gap), barWidth));
            }
        }

        const unlisten = listen<number>(eventName, (event) => {
            const raw = Math.min(event.payload * 6, 1.0);
            audioLevelRef.current = Math.sqrt(raw);
        });

        return () => {
            unlisten.then(f => f());
        };
    }, [eventName]);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        const render = () => {
            // Smooth the audio level
            if (isRecording) {
                smoothedLevelRef.current += (audioLevelRef.current - smoothedLevelRef.current) * 0.25;
            } else {
                smoothedLevelRef.current *= 0.9;
                audioLevelRef.current *= 0.9;
            }

            const level = smoothedLevelRef.current;

            ctx.clearRect(0, 0, canvas.width, canvas.height);

            const centerX = canvas.width / 2;
            const centerY = canvas.height / 2;

            // Draw bars
            const maxHeight = 80;
            barsRef.current.forEach((bar) => {
                bar.update(level, maxHeight);
                // Low saturation color - neutral-400 with hint of chinese-indigo
                const alpha = 0.3 + level * 0.4;
                bar.draw(ctx, centerY, `rgba(22, 97, 171, ${alpha})`);
            });

            // Draw center indicator line
            if (isRecording) {
                ctx.fillStyle = `rgba(22, 97, 171, ${0.6 + level * 0.4})`;
                ctx.fillRect(centerX - 1, centerY - 20, 2, 40);
            }

            animationFrameRef.current = requestAnimationFrame(render);
        };

        render();

        return () => {
            if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
        };
    }, [isRecording]);

    return (
        <canvas
            ref={canvasRef}
            width={300}
            height={200}
            className={`pointer-events-none ${className || "w-32 h-24"}`}
        />
    );
}

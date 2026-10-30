import { Theme } from "./api";
export type { Theme };

export interface ThemeInfo {
  id: Theme;
  label: string;
  tagline: string;
  category: "Auto" | "Neutral" | "Vibrant" | "Light" | "Warm";
  description: string;
  icon: string;
  colors: {
    bg: string;
    card: string;
    border: string;
    text: string;
    accent: string;
    accentGlow: string;
    secondaryAccent?: string;
  };
}

export const THEMES: ThemeInfo[] = [
  {
    id: "system",
    label: "System",
    tagline: "Dynamic OS Match",
    category: "Auto",
    description: "Follows your operating system's light or dark appearance preference in real time.",
    icon: "💻",
    colors: {
      bg: "#161616",
      card: "#222222",
      border: "#3a3a3a",
      text: "#d4d4d4",
      accent: "#38bdf8",
      accentGlow: "rgba(56, 189, 248, 0.3)",
      secondaryAccent: "#818cf8",
    },
  },
  {
    id: "dark",
    label: "Studio Dark",
    tagline: "Colorist Standard",
    category: "Neutral",
    description: "Precision neutral dark charcoal (#161616) — zero chromatic bias for critical grading.",
    icon: "🌙",
    colors: {
      bg: "#161616",
      card: "#242424",
      border: "#3d3d3d",
      text: "#d4d4d4",
      accent: "#0ea5e9",
      accentGlow: "rgba(14, 165, 233, 0.35)",
      secondaryAccent: "#38bdf8",
    },
  },
  {
    id: "light",
    label: "Studio Light",
    tagline: "High-Key Clean",
    category: "Light",
    description: "Crisp bright surfaces with sapphire blue accents for daytime editing & print matching.",
    icon: "☀️",
    colors: {
      bg: "#f4f5f8",
      card: "#ffffff",
      border: "#e2e8f0",
      text: "#0f172a",
      accent: "#2563eb",
      accentGlow: "rgba(37, 99, 235, 0.2)",
      secondaryAccent: "#0284c7",
    },
  },
  {
    id: "slate",
    label: "Cool Slate",
    tagline: "Oceanic Navy",
    category: "Modern" as const as "Vibrant",
    description: "Deep oceanic blue-slate tones with bright cyan highlights for a sleek pro studio vibe.",
    icon: "🔷",
    colors: {
      bg: "#0b1329",
      card: "#111c38",
      border: "#1e293b",
      text: "#e2e8f0",
      accent: "#38bdf8",
      accentGlow: "rgba(56, 189, 248, 0.35)",
      secondaryAccent: "#06b6d4",
    },
  },
  {
    id: "warm",
    label: "Warm Darkroom",
    tagline: "Analog Amber & Film",
    category: "Warm",
    description: "Rich dark espresso and mahogany tones with glowing amber gold; gentle on the eyes.",
    icon: "🕯️",
    colors: {
      bg: "#181412",
      card: "#231c19",
      border: "#3a2e28",
      text: "#e7e5e4",
      accent: "#f59e0b",
      accentGlow: "rgba(245, 158, 11, 0.35)",
      secondaryAccent: "#fbbf24",
    },
  },
  {
    id: "nordic",
    label: "Nordic Frost",
    tagline: "Arctic Petrol & Mint",
    category: "Vibrant",
    description: "Petrol midnight navy with refreshing arctic ice-cyan accents — razor-sharp & modern.",
    icon: "❄️",
    colors: {
      bg: "#07131b",
      card: "#0d202d",
      border: "#18374a",
      text: "#e0f2fe",
      accent: "#06b6d4",
      accentGlow: "rgba(6, 182, 212, 0.35)",
      secondaryAccent: "#22d3ee",
    },
  },
  {
    id: "cyber",
    label: "Midnight Violet",
    tagline: "Neon Cyber Studio",
    category: "Vibrant",
    description: "Deep obsidian violet surfaces with electric purple & neon orchid accents for digital creators.",
    icon: "🔮",
    colors: {
      bg: "#0e091b",
      card: "#17102c",
      border: "#2a1d4f",
      text: "#f3e8ff",
      accent: "#a855f7",
      accentGlow: "rgba(168, 85, 247, 0.35)",
      secondaryAccent: "#c084fc",
    },
  },
  {
    id: "emerald",
    label: "Forest Sage",
    tagline: "Botanical Moss & Pine",
    category: "Vibrant",
    description: "Deep pine & moss darkroom with radiant emerald green accents, ideal for nature photography.",
    icon: "🌿",
    colors: {
      bg: "#08140f",
      card: "#0f231a",
      border: "#183b2c",
      text: "#ecfdf5",
      accent: "#10b981",
      accentGlow: "rgba(16, 185, 129, 0.35)",
      secondaryAccent: "#34d399",
    },
  },
  {
    id: "oled",
    label: "OLED Obsidian",
    tagline: "100% True Black",
    category: "Neutral",
    description: "True pure #000000 pitch-black canvas with electric indigo contrast for OLED/HDR monitors.",
    icon: "⚫",
    colors: {
      bg: "#000000",
      card: "#121214",
      border: "#27272a",
      text: "#f4f4f5",
      accent: "#6366f1",
      accentGlow: "rgba(99, 102, 241, 0.4)",
      secondaryAccent: "#818cf8",
    },
  },
];

export function getStoredTheme(): Theme {
  try {
    const t = localStorage.getItem("openlight-theme") as Theme | null;
    if (t && ["system", "dark", "light", "slate", "warm", "nordic", "cyber", "emerald", "oled"].includes(t)) {
      return t;
    }
  } catch (_) {}
  return "system";
}

export function isSystemDark(): boolean {
  if (typeof window === "undefined") return true;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyTheme(theme: Theme) {
  try {
    localStorage.setItem("openlight-theme", theme);
  } catch (_) {}

  const root = document.documentElement;
  root.setAttribute("data-theme", theme);

  const dark = isSystemDark();
  const isLight = theme === "light" || (theme === "system" && !dark);

  let meta = document.querySelector('meta[name="color-scheme"]') as HTMLMetaElement | null;
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "color-scheme";
    document.head.appendChild(meta);
  }
  meta.content = theme === "system" ? "light dark" : isLight ? "light" : "dark";
}

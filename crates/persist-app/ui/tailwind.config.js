/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "#191C16",
        panel: "#20241C",
        panel2: "#272C22",
        line: "#363B2C",
        line2: "#454B38",
        ink: "#E8E4D5",
        muted: "#9C9F8B",
        faint: "#6C7060",
        moss: "#93AC6A",
        mossdeep: "#6E8549",
        clay: "#C67B4B",
        water: "#6E9E93",
        rust: "#C2564A",
      },
      fontFamily: {
        sans: ["IBM Plex Sans", "system-ui", "sans-serif"],
        mono: ["IBM Plex Mono", "ui-monospace", "monospace"],
        pixel: ["Silkscreen", "IBM Plex Mono", "monospace"],
      },
      boxShadow: {
        pixel: "3px 3px 0 0 rgba(0, 0, 0, 0.5)",
        pixelmoss: "3px 3px 0 0 #6E8549",
        pixelclay: "3px 3px 0 0 #8A4F2C",
      },
      borderRadius: {
        DEFAULT: "3px",
      },
      keyframes: {
        "fade-in": {
          from: { opacity: "0", transform: "translateY(4px)" },
          to: { opacity: "1", transform: "translateY(0)" },
        },
        "slide-in": {
          from: { opacity: "0", transform: "translateX(12px)" },
          to: { opacity: "1", transform: "translateX(0)" },
        },
      },
      animation: {
        "fade-in": "fade-in 180ms ease-out both",
        "slide-in": "slide-in 160ms ease-out both",
      },
    },
  },
  plugins: [],
};

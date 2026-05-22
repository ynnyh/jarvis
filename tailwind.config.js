/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./desktop/**/*.html",
    "./desktop/**/*.vue",
    "./desktop/**/*.ts",
  ],
  theme: {
    extend: {
      colors: {
        jarvis: {
          dark: '#1a1a2e',
          primary: '#00d4ff',
          glass: 'rgba(26, 26, 46, 0.85)',
        }
      }
    },
  },
  plugins: [],
}

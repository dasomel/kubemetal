/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      // DESIGN.md(루트)와 1:1로 유지할 것. 변경 시 `npx @google/design.md export
      // --format json-tailwind DESIGN.md`로 정합을 확인한다. DESIGN.md가 유일한 소스다.
      colors: {
        primary: '#6d3fe0',
        primaryStrong: '#5b2fc7',
        inverse: '#ffffff',
        base: '#fafafa',
        surface: '#ffffff',
        surfaceRaised: '#f4f4f5',
        hairline: '#0f1115',
        ink: '#23262b',
        inkMuted: '#52565c',
        inkFaint: '#686c73',
        success: '#15803d',
        warning: '#b45309',
        danger: '#b91c1c',
        dangerStrong: '#9b1c1c',
      },
      fontFamily: {
        // display/heading/label/body/bodyStrong/caption은 DESIGN.md typography 토큰과
        // 동일한 한국어 대응 산세리프 스택을 공유한다. sans는 body 기본값의 별칭.
        sans: [
          '-apple-system',
          'BlinkMacSystemFont',
          '"Apple SD Gothic Neo"',
          'Pretendard',
          '"SF Pro Text"',
          '"SF Pro Display"',
          '"Segoe UI"',
          'Roboto',
          'Helvetica',
          'Arial',
          'sans-serif',
        ],
        mono: ['"SF Mono"', '"JetBrains Mono"', 'ui-monospace', 'Menlo', 'monospace'],
      },
      fontSize: {
        display: ['22px', { lineHeight: '28px', letterSpacing: '-0.02em', fontWeight: '700' }],
        heading: ['15px', { lineHeight: '20px', letterSpacing: '-0.01em', fontWeight: '600' }],
        label: ['11px', { lineHeight: '14px', letterSpacing: '0.08em', fontWeight: '600' }],
        body: ['14px', { lineHeight: '20px', fontWeight: '400' }],
        bodyStrong: ['14px', { lineHeight: '20px', fontWeight: '600' }],
        caption: ['12px', { lineHeight: '16px', fontWeight: '500' }],
        metric: ['34px', { lineHeight: '38px', letterSpacing: '-0.01em', fontWeight: '600' }],
      },
      borderRadius: {
        sm: '6px',
        md: '8px',
        lg: '12px',
        xl: '16px',
      },
      spacing: {
        xs: '4px',
        sm: '8px',
        md: '12px',
        lg: '16px',
        xl: '24px',
        '2xl': '32px',
        '3xl': '40px',
      },
      boxShadow: {
        // DESIGN.md "Elevation & Depth" 참조: 표면은 밝기 차이로, 그림자는 최소한만.
        panel: '0 1px 2px rgba(15,17,21,0.04), 0 10px 24px -10px rgba(15,17,21,0.12)',
      },
    },
  },
  plugins: [],
}

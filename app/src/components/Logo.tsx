/** Nebula 品牌标志(星云徽章)。 */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 1024 1024"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient
          id="nebula-logo-bg"
          x1="140"
          y1="120"
          x2="900"
          y2="920"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0" stopColor="#5B8CFF" />
          <stop offset="1" stopColor="#8B5CF6" />
        </linearGradient>
      </defs>
      <rect x="96" y="96" width="832" height="832" rx="200" fill="url(#nebula-logo-bg)" />
      <ellipse
        cx="512"
        cy="520"
        rx="360"
        ry="150"
        fill="none"
        stroke="#fff"
        strokeWidth="16"
        strokeOpacity="0.28"
        transform="rotate(-24 512 520)"
      />
      <g fill="#fff">
        <rect x="356" y="500" width="330" height="150" rx="75" />
        <circle cx="440" cy="502" r="92" />
        <circle cx="558" cy="470" r="122" />
        <circle cx="652" cy="522" r="82" />
      </g>
      <path
        d="M728 300 L742 336 L778 350 L742 364 L728 400 L714 364 L678 350 L714 336 Z"
        fill="#fff"
      />
      <circle cx="300" cy="300" r="16" fill="#fff" fillOpacity="0.85" />
      <circle cx="792" cy="700" r="12" fill="#fff" fillOpacity="0.7" />
    </svg>
  );
}

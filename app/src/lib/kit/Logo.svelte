<!-- The app's lamp, lit: the same drawing as the app icon
     (src-tauri/icons/source.svg), without the tile behind it. -->
<script lang="ts">
  let { size = 20 }: { size?: number } = $props();
</script>

<svg xmlns="http://www.w3.org/2000/svg" viewBox="400 220 224 540" height={size} width={size * (224 / 540)} aria-hidden="true">
  <defs>
    <linearGradient id="logo-flame" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffd978" />
      <stop offset="1" stop-color="#f08a24" />
    </linearGradient>
  </defs>
  <path d="M512 235C468 320 436 372 436 430a76 76 0 0 0 152 0C588 372 556 320 512 235Z" fill="url(#logo-flame)" />
  <path d="M512 350C494 387 482 410 482 434a30 30 0 0 0 60 0C542 410 530 387 512 350Z" fill="#fff1bd" />
  <rect x="446" y="522" width="132" height="24" rx="12" fill="#f4f1ea" />
  <rect x="498" y="546" width="28" height="170" fill="#f4f1ea" />
  <rect x="416" y="716" width="192" height="28" rx="14" fill="#f4f1ea" />
</svg>

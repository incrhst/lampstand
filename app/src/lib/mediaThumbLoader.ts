// The one loader of media thumbnails, shared by the library and service rows
// and by Present, so a file is decoded once however many places show it.

import { request } from "./api";
import { thumbnailLoader } from "./mediaThumbs";

export const loadMediaThumb = thumbnailLoader(async (blob, name, width) =>
  request({ area: "Library", request: { MediaThumbnail: { blob, name, width } } }, "Thumbnail"),
);

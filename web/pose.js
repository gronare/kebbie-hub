export const POSES = ["phone", "stack", "split", "land", "desk"];

export function choosePose({ width, height, strip = false }) {
  if (width >= 1024) return "desk";
  if (width >= 840 && height >= 500 && width > height) return "split";
  if (strip) return "stack";
  if (width >= 800 && width > height && height >= 330) return "land";
  if (width >= 600 && height > width) return "stack";
  return "phone";
}

export const overviewPose = (pose, portrait) => pose === "stack" || (pose === "phone" && portrait);

export const railSide = (angle) => (angle === 270 ? "left" : "right");

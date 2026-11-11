export function outOfRangeScroll(scrollY, scrollHeight, viewportHeight) {
  const max = Math.max(0, scrollHeight - viewportHeight);
  return scrollY > max ? max : null;
}

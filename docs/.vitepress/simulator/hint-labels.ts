/** The same prefix-free label plan as api::hint, shared by both previews. */
export function hintLabels(characters: string, count: number, reverse = false): string[] {
  const keys = [...characters.toLowerCase()]
  const radix = keys.length
  if (radix < 2 || count < 1) return []
  const reserved = Math.ceil(Math.max(0, count - radix) / (radix - 1))
  const singles = radix - reserved
  let width = 1, capacity = radix
  while (capacity < count) { width++; capacity *= radix }
  return Array.from({ length: count }, (_, index) => {
    if (!reverse && reserved < radix) {
      if (index < singles) return keys[index]
      const pair = index - singles
      return keys[singles + Math.floor(pair / radix)] + keys[pair % radix]
    }
    let label = ''
    for (let digit = 0; digit < width; digit++) {
      const character = keys[index % radix]
      label = reverse ? label + character : character + label
      index = Math.floor(index / radix)
    }
    return label
  })
}

export function validQuickSwitchCharacters(characters: unknown, trigger: string): characters is string {
  if (typeof characters !== 'string') return false
  const keys = [...characters.toLowerCase()]
  return keys.length >= 2 && new Set(keys).size === keys.length
    && [...characters].every(character => [...character.toLowerCase()].length === 1)
    && !keys.includes(trigger.toLowerCase()) && !/[\s\u0000-\u001f\u007f]/u.test(characters)
}

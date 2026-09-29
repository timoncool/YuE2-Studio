/**
 * The dates a list shows beside a row: when the thing was made, and - when it was touched
 * afterwards - when it last changed. Written the way the person's language writes it, and
 * short enough to sit in the narrow right-hand column of a track row.
 */
export function stampOf(value: Date | number | null | undefined, language: string): string {
    const date = value instanceof Date ? value : new Date(value ?? 0);
    if (Number.isNaN(date.getTime()) || date.getTime() === 0) return '';
    return date.toLocaleString(language, {
        day: '2-digit',
        month: '2-digit',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
    });
}

/** A minute of slack: a save right after making something is not a change worth showing. */
const A_MINUTE = 60_000;

/** Was it touched after it was made? Only then is there a change to show. */
export function changedAfter(createdAt: Date | number | null | undefined, updatedAt: Date | number | null | undefined): boolean {
    const made = createdAt instanceof Date ? createdAt.getTime() : Number(createdAt ?? 0);
    const touched = updatedAt instanceof Date ? updatedAt.getTime() : Number(updatedAt ?? 0);
    if (!made || !touched) return false;
    return touched - made > A_MINUTE;
}

import { describe, expect, it } from 'vitest';
import { orderSessions, type SortOrder } from './workspaces';

/** Three sessions, newest first by when they were made. */
const newest = { id: 'newest', name: 'Яблоко', createdAt: 300, updatedAt: 300 };
const middle = { id: 'middle', name: 'Банан', createdAt: 200, updatedAt: 500 };
const oldest = { id: 'oldest', name: 'Апельсин', createdAt: 100, updatedAt: 150 };

/** Newest first, which is how the lists start. */
const byCreated: SortOrder = { by: 'created', descending: true };

describe('orderSessions', () => {
    it('reads newest first when no session is open', () => {
        expect(orderSessions([oldest, newest, middle], null, byCreated).map((session) => session.id))
            .toEqual(['newest', 'middle', 'oldest']);
    });

    it('lifts the open session to the very first row', () => {
        expect(orderSessions([newest, middle, oldest], 'oldest', byCreated).map((session) => session.id))
            .toEqual(['oldest', 'newest', 'middle']);
        expect(orderSessions([newest, middle, oldest], 'middle', byCreated).map((session) => session.id))
            .toEqual(['middle', 'newest', 'oldest']);
    });

    it('reads by name when the person asks for it, and either way round', () => {
        expect(orderSessions([newest, middle, oldest], null, { by: 'name', descending: false }).map((session) => session.id))
            .toEqual(['oldest', 'middle', 'newest']);
        expect(orderSessions([newest, middle, oldest], null, { by: 'name', descending: true }).map((session) => session.id))
            .toEqual(['newest', 'middle', 'oldest']);
        expect(orderSessions([newest, middle, oldest], null, { by: 'created', descending: false }).map((session) => session.id))
            .toEqual(['oldest', 'middle', 'newest']);
    });

    it('reads by the day it was last touched, not the day it was made', () => {
        expect(orderSessions([newest, middle, oldest], null, { by: 'updated', descending: true }).map((session) => session.id))
            .toEqual(['middle', 'newest', 'oldest']);
        expect(orderSessions([newest, middle, oldest], null, { by: 'updated', descending: false }).map((session) => session.id))
            .toEqual(['oldest', 'newest', 'middle']);
    });

    it('falls back to the day it was made when nothing touched it since', () => {
        const untouched = { id: 'untouched', name: 'Виноград', createdAt: 400 };
        expect(orderSessions([newest, untouched], null, { by: 'updated', descending: true }).map((session) => session.id))
            .toEqual(['untouched', 'newest']);
    });

    it('keeps the current session first whichever order is chosen', () => {
        expect(orderSessions([newest, middle, oldest], 'newest', { by: 'name', descending: false }).map((session) => session.id))
            .toEqual(['newest', 'oldest', 'middle']);
    });

    it('leaves the list it was given alone', () => {
        const given = [newest, middle, oldest];
        orderSessions(given, 'oldest', byCreated);
        expect(given.map((session) => session.id)).toEqual(['newest', 'middle', 'oldest']);
    });

    it('keeps an unknown current id from breaking the order', () => {
        expect(orderSessions([oldest, newest], 'gone', byCreated).map((session) => session.id))
            .toEqual(['newest', 'oldest']);
    });
});

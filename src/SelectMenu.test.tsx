import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import SelectMenu from './SelectMenu';

afterEach(cleanup);

function setup(disabled = false) {
  const change = vi.fn();
  render(<div className="popover"><SelectMenu label="Workspace" value="personal" className="workspace-picker" onChange={change} disabled={disabled}
    options={[{ value: 'personal', label: 'Personal', content: <>Free Personal</> }, { value: 'team', label: 'Team', content: <>Pro Team</> }]}>
    Personal
  </SelectMenu><button>Settings</button></div>);
  return { change, trigger: screen.getByRole('combobox') };
}

test('keyboard selection, Escape, and Tab keep expected focus', async () => {
  const { trigger, change } = setup();
  trigger.focus();
  await userEvent.keyboard('{ArrowDown}');
  fireEvent(window, new Event('resize'));
  expect(screen.getByRole('listbox')).toBeTruthy();
  expect(screen.getByRole('option', { name: 'Personal' }).getAttribute('aria-selected')).toBe('true');
  await userEvent.keyboard('{End}{Enter}');
  expect(change).toHaveBeenCalledWith('team');
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(document.activeElement).toBe(trigger);
  await userEvent.keyboard('{Enter}{Escape}');
  expect(screen.queryByRole('listbox')).toBeNull();
  await userEvent.keyboard('{Enter}{Tab}');
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Settings' }));
});

test('outside clicks dismiss without selecting and disabled triggers stay closed', async () => {
  const { trigger, change } = setup();
  await userEvent.click(trigger);
  await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(change).not.toHaveBeenCalled();
  cleanup();
  const disabled = setup(true);
  await userEvent.click(disabled.trigger);
  expect(screen.queryByRole('listbox')).toBeNull();
});

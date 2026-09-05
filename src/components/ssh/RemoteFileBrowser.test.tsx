/**
 * Component tests for RemoteFileBrowser — the SSH remote file manager.
 *
 * Covers the wire-up added in 021ef98c: loading/listing, file selection,
 * Edit/Save/Cancel on the draft content, Rename with list refresh, and the
 * error-handling paths (every handler surfaces failures through the toast
 * stack instead of throwing).
 *
 * Only the `remoteFiles` invoke wrappers are mocked — the component under
 * test renders inside a real <ToastProvider> so toast assertions go through
 * the actual toast stack (same pattern as ToastContext.test.tsx).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { RemoteFileBrowser } from './RemoteFileBrowser';
import { ToastProvider, useToast } from '../../contexts/ToastContext';
import type { SshServer } from '../../lib/ssh';
import type { FileEntry, FileContent } from '../../lib/code';

vi.mock('../../lib/remoteFiles', () => ({
  listRemoteFiles: vi.fn(),
  readRemoteFile: vi.fn(),
  saveRemoteFile: vi.fn(),
  createRemoteDirectory: vi.fn(),
  deleteRemoteFile: vi.fn(),
  renameRemoteFile: vi.fn(),
}));

import {
  listRemoteFiles,
  readRemoteFile,
  saveRemoteFile,
  renameRemoteFile,
} from '../../lib/remoteFiles';

const server: SshServer = {
  id: 'srv-1',
  name: 'Test VPS',
  host: 'example.com',
  port: 22,
  username: 'deploy',
  keyPath: null,
  authType: 'key',
  createdAt: 0,
  lastConnectedAt: null,
};

const entries: FileEntry[] = [
  { name: 'src', path: 'src', isDirectory: true, size: 0 },
  { name: 'app.js', path: 'app.js', isDirectory: false, size: 234 },
];

const fileContent: FileContent = {
  content: 'console.log("hello");',
  isBinary: false,
  isTruncated: false,
  size: 21,
  language: 'javascript',
};

const onBack = vi.fn();

/** Renders the shared toast stack the way App.tsx does (from `useToast().toasts`). */
function ToastStack() {
  const { toasts } = useToast();
  return (
    <div className="toast-container">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.type}`}>
          {t.message}
        </div>
      ))}
    </div>
  );
}

function renderBrowser() {
  return render(
    <ToastProvider>
      <RemoteFileBrowser server={server} onBack={onBack} />
      <ToastStack />
    </ToastProvider>
  );
}

/** Loads the list, opens app.js and waits for its content to render. */
async function openAppJs() {
  renderBrowser();
  fireEvent.click(await screen.findByRole('button', { name: /app\.js/ }));
  await screen.findByText('console.log("hello");');
}

describe('RemoteFileBrowser loading and selection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listRemoteFiles).mockResolvedValue(entries);
    vi.mocked(readRemoteFile).mockResolvedValue(fileContent);
    vi.mocked(saveRemoteFile).mockResolvedValue(undefined);
    vi.mocked(renameRemoteFile).mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('lists remote files on mount', async () => {
    renderBrowser();

    expect(await screen.findByText('app.js')).toBeInTheDocument();
    expect(screen.getByText('src')).toBeInTheDocument();
    expect(listRemoteFiles).toHaveBeenCalledWith('srv-1', '/home');
  });

  it('navigates into a directory when a directory entry is clicked', async () => {
    renderBrowser();
    fireEvent.click(await screen.findByRole('button', { name: /src/ }));

    await waitFor(() => {
      expect(listRemoteFiles).toHaveBeenCalledWith('srv-1', '/home/src');
    });
  });

  it('opens a file, renders its content and tracks its path', async () => {
    renderBrowser();
    fireEvent.click(await screen.findByRole('button', { name: /app\.js/ }));

    expect(await screen.findByText('console.log("hello");')).toBeInTheDocument();
    expect(readRemoteFile).toHaveBeenCalledWith('srv-1', '/home/app.js');
    // The header actions appear once a file is selected.
    expect(screen.getByRole('button', { name: 'Edit' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rename' })).toBeInTheDocument();
  });

  it('shows the binary placeholder and hides Edit for binary files', async () => {
    vi.mocked(readRemoteFile).mockResolvedValue({
      ...fileContent,
      isBinary: true,
      content: '',
    });
    renderBrowser();
    fireEvent.click(await screen.findByRole('button', { name: /app\.js/ }));

    expect(await screen.findByText('Binary file — cannot display.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument();
  });

  it('shows the too-large placeholder and hides Edit for truncated files', async () => {
    vi.mocked(readRemoteFile).mockResolvedValue({
      ...fileContent,
      isTruncated: true,
    });
    renderBrowser();
    fireEvent.click(await screen.findByRole('button', { name: /app\.js/ }));

    expect(await screen.findByText(/File is too large/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument();
  });
});

describe('RemoteFileBrowser edit, save and cancel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listRemoteFiles).mockResolvedValue(entries);
    vi.mocked(readRemoteFile).mockResolvedValue(fileContent);
    vi.mocked(saveRemoteFile).mockResolvedValue(undefined);
    vi.mocked(renameRemoteFile).mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('Edit opens a textarea seeded with the file content', async () => {
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));

    const textarea = screen.getByLabelText('Editing app.js');
    expect(textarea).toHaveValue('console.log("hello");');
  });

  it('Save persists the edited draft, exits edit mode and shows the new content', async () => {
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    fireEvent.change(screen.getByLabelText('Editing app.js'), {
      target: { value: 'console.log("updated");' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(saveRemoteFile).toHaveBeenCalledWith(
        'srv-1',
        '/home/app.js',
        'console.log("updated");'
      );
    });
    // Success feedback through the real toast stack.
    expect(await screen.findByText('File saved')).toBeInTheDocument();
    // Edit mode closed; the viewer shows the saved content.
    expect(screen.queryByLabelText('Editing app.js')).not.toBeInTheDocument();
    expect(screen.getByText('console.log("updated");')).toBeInTheDocument();
    // Save intentionally refreshes local state only — no list reload.
    expect(listRemoteFiles).toHaveBeenCalledTimes(1);
  });

  it('Cancel discards the draft without calling save', async () => {
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    fireEvent.change(screen.getByLabelText('Editing app.js'), {
      target: { value: 'changed but unsaved' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(saveRemoteFile).not.toHaveBeenCalled();
    expect(screen.queryByLabelText('Editing app.js')).not.toBeInTheDocument();
    expect(screen.getByText('console.log("hello");')).toBeInTheDocument();
  });

  it('keeps edit mode open when saving fails and surfaces the error', async () => {
    vi.mocked(saveRemoteFile).mockRejectedValue(new Error('ssh write failed'));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    fireEvent.change(screen.getByLabelText('Editing app.js'), {
      target: { value: 'will not persist' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(document.querySelector('.toast-error')).not.toBeNull();
    });
    // Draft is preserved for a retry.
    expect(screen.getByLabelText('Editing app.js')).toBeInTheDocument();
    expect(screen.queryByText('File saved')).not.toBeInTheDocument();
  });
});

describe('RemoteFileBrowser rename and refresh', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listRemoteFiles).mockResolvedValue(entries);
    vi.mocked(readRemoteFile).mockResolvedValue(fileContent);
    vi.mocked(saveRemoteFile).mockResolvedValue(undefined);
    vi.mocked(renameRemoteFile).mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renames the selected file and refreshes the list', async () => {
    vi.stubGlobal('prompt', vi.fn().mockReturnValue('renamed.js'));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    await waitFor(() => {
      expect(renameRemoteFile).toHaveBeenCalledWith('srv-1', '/home/app.js', '/home/renamed.js');
    });
    // The list is reloaded after a rename (mount call + refresh call).
    await waitFor(() => {
      expect(listRemoteFiles).toHaveBeenCalledTimes(2);
    });
    expect(await screen.findByText('File renamed')).toBeInTheDocument();
    // Selection tracks the new name.
    expect(screen.getByText('renamed.js')).toBeInTheDocument();
  });

  it('does not rename when the prompt is cancelled', async () => {
    vi.stubGlobal('prompt', vi.fn().mockReturnValue(null));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    expect(renameRemoteFile).not.toHaveBeenCalled();
    expect(screen.queryByText('File renamed')).not.toBeInTheDocument();
  });

  it('does not rename when the name is unchanged', async () => {
    vi.stubGlobal('prompt', vi.fn().mockReturnValue('app.js'));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    expect(renameRemoteFile).not.toHaveBeenCalled();
  });

  it('shows an error toast when renaming fails', async () => {
    vi.stubGlobal('prompt', vi.fn().mockReturnValue('renamed.js'));
    vi.mocked(renameRemoteFile).mockRejectedValue(new Error('mv failed'));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    await waitFor(() => {
      expect(document.querySelector('.toast-error')).not.toBeNull();
    });
    expect(screen.queryByText('File renamed')).not.toBeInTheDocument();
  });
});

describe('RemoteFileBrowser error handling', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(saveRemoteFile).mockResolvedValue(undefined);
    vi.mocked(renameRemoteFile).mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows an error toast when listing fails', async () => {
    vi.mocked(listRemoteFiles).mockRejectedValue(new Error('ssh down'));
    renderBrowser();

    await waitFor(() => {
      expect(document.querySelector('.toast-error')).not.toBeNull();
    });
    expect(screen.getByText('No files in this directory.')).toBeInTheDocument();
  });

  it('shows an error toast when reading a file fails', async () => {
    vi.mocked(listRemoteFiles).mockResolvedValue(entries);
    vi.mocked(readRemoteFile).mockRejectedValue(new Error('cat failed'));
    renderBrowser();
    fireEvent.click(await screen.findByRole('button', { name: /app\.js/ }));

    await waitFor(() => {
      expect(document.querySelector('.toast-error')).not.toBeNull();
    });
    // No stale content is shown after a failed read.
    expect(screen.getByText('Select a file to view its content.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument();
  });

  it('re-lists files after a failed read when the user retries the same file', async () => {
    vi.mocked(listRemoteFiles).mockResolvedValue(entries);
    vi.mocked(readRemoteFile)
      .mockRejectedValueOnce(new Error('cat failed'))
      .mockResolvedValueOnce(fileContent);
    renderBrowser();
    fireEvent.click(await screen.findByRole('button', { name: /app\.js/ }));
    await waitFor(() => {
      expect(document.querySelector('.toast-error')).not.toBeNull();
    });

    // A second click recovers — the failure was surfaced, not fatal.
    fireEvent.click(screen.getByRole('button', { name: /app\.js/ }));
    expect(await screen.findByText('console.log("hello");')).toBeInTheDocument();
  });
});

describe('RemoteFileBrowser busy state', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listRemoteFiles).mockResolvedValue(entries);
    vi.mocked(readRemoteFile).mockResolvedValue(fileContent);
    vi.mocked(saveRemoteFile).mockResolvedValue(undefined);
    vi.mocked(renameRemoteFile).mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('disables Save and shows Saving... while the save is in flight', async () => {
    vi.mocked(saveRemoteFile).mockImplementation(() => new Promise<void>(() => {}));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    const busy = await screen.findByRole('button', { name: 'Saving...' });
    expect(busy).toBeDisabled();
    // Rename can't start mid-save either.
    expect(screen.getByRole('button', { name: 'Rename' })).toBeDisabled();

    // Busy clears in-flight completion would need the deferred promise; here
    // the promise never resolves, so just assert the state persists.
    expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument();
  });

  it('ignores double clicks while a save is in flight', async () => {
    vi.mocked(saveRemoteFile).mockImplementation(() => new Promise<void>(() => {}));
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await screen.findByRole('button', { name: 'Saving...' });
    // Second click on the busy (disabled) button must not queue another save.
    fireEvent.click(screen.getByRole('button', { name: 'Saving...' }));

    expect(saveRemoteFile).toHaveBeenCalledTimes(1);
  });

  it('disables Rename while a rename is in flight and ignores repeat clicks', async () => {
    vi.stubGlobal('prompt', vi.fn().mockReturnValue('renamed.js'));
    vi.mocked(renameRemoteFile).mockImplementation(() => new Promise<void>(() => {}));
    await openAppJs();

    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const busy = await screen.findByRole('button', { name: 'Renaming...' });
    expect(busy).toBeDisabled();
    fireEvent.click(busy);

    expect(renameRemoteFile).toHaveBeenCalledTimes(1);
  });

  it('re-enables Save after a failure and allows a retry', async () => {
    vi.mocked(saveRemoteFile)
      .mockRejectedValueOnce(new Error('ssh write failed'))
      .mockResolvedValueOnce(undefined);
    await openAppJs();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    // In-flight: busy and disabled.
    await screen.findByRole('button', { name: 'Saving...' });
    // Failure clears the busy state (finally) and surfaces the error.
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
    });
    expect(document.querySelector('.toast-error')).not.toBeNull();

    // Retry succeeds.
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(saveRemoteFile).toHaveBeenCalledTimes(2);
    });
    expect(await screen.findByText('File saved')).toBeInTheDocument();
  });
});

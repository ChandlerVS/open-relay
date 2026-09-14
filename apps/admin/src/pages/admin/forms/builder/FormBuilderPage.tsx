import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ArrowLeft, Eye, EyeOff } from "lucide-react";
import {
  PACKED_SUBDIVISIONS,
  ShadowForm,
  type PublicFormDto,
} from "@open-relay/form-renderer";
import {
  Alert,
  AlertDescription,
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  ConfirmDialog,
  Skeleton,
} from "@open-relay/ui";
import { api } from "../../../../lib/api/client";
import { usePermissions } from "../../../../lib/auth/usePermissions";
import { useStorageConfig } from "../../../../lib/storage/useStorage";
import { useForm } from "../../../../lib/forms/useForms";
import { useResolvedFormTheme } from "../../../../lib/formThemes/useThemes";
import { useUpdateForm } from "../../../../lib/forms/useFormMutations";
import { useTheme } from "../../../../lib/theme/useTheme";
import { Canvas } from "./Canvas";
import { Inspector } from "./Inspector";
import { Palette } from "./Palette";
import { SelectionToolbar } from "./SelectionToolbar";
import { validateLayout } from "./validate";
import { extractBlock, prepareForPaste, removeMany, type PasteOutcome } from "./blockOps";
import { serializeElements, writeLocalClipboard } from "./clipboard";
import { useSelection } from "./useSelection";
import { useBuilderClipboard } from "./useBuilderClipboard";
import {
  controllerCandidates,
  countryFieldCandidates,
  elementKey,
  isCountryField,
  isInsideRow,
  newCustomElement,
  newRowElements,
  renameCountryReferences,
  stripCountryReferences,
  newDecorationElement,
  newId,
  newStandardElement,
  renameRuleReferences,
  stripIds,
  usedStandardKeys,
  withIds,
  withRule,
  type BuilderElement,
  type CustomTypeName,
  type FormElement,
  type VisibilityRule,
} from "./model";

/** Deleting more than a handful at once, with no undo, is worth a confirm. */
const CONFIRM_DELETE_ABOVE = 3;

/**
 * Pins the Add and Settings columns to the viewport, so a form long enough to
 * run off the bottom of the screen doesn't strand the palette and the inspector
 * several screens above where you're working.
 *
 * Three things here are load-bearing:
 *
 * - **`self-start`.** A grid item is stretched to the row height by default, and
 *   a sticky box that exactly fills its containing block has nowhere to travel —
 *   it computes, moves nothing, and reads as broken. Shrinking the card to its
 *   content leaves the rest of the (canvas-height) grid area as its scroll range.
 * - **The breakpoint prefix.** Below it the grid is a single column and the cards
 *   stack, where a pinned panel would sit on top of the content it is meant to
 *   sit beside. The prefix differs with `showPreview`, so both variants are
 *   spelled out in full rather than interpolated: Tailwind scans source text, and
 *   a `${bp}:sticky` template would emit no CSS at all.
 * - **Not applying either of these to the Fields card.** An `overflow-*` on the
 *   canvas column would become a scrollable, clipping ancestor of the draggable
 *   rows — dnd-kit would retarget auto-scroll to it, and since rows are
 *   transformed in place rather than drawn in a `DragOverlay`, a dragged row
 *   would be clipped at the card's edge.
 */
const SIDE_PANEL = {
  lg: "lg:sticky lg:top-6 lg:self-start lg:max-h-[calc(100vh_-_3rem)] lg:flex lg:flex-col lg:overflow-hidden",
  xl: "xl:sticky xl:top-6 xl:self-start xl:max-h-[calc(100vh_-_3rem)] xl:flex xl:flex-col xl:overflow-hidden",
} as const;

/**
 * The pinned card's scrolling body — neither panel fits a laptop viewport (the
 * palette alone is ~32 buttons), so the card caps its height and the content
 * scrolls under a header that stays put.
 *
 * `min-h-0` is what actually makes that work: a flex child's automatic minimum
 * size is its content height, so without it the card's `max-h` is ignored and
 * the scroller never engages.
 */
const SIDE_PANEL_BODY = {
  lg: "lg:min-h-0 lg:flex-1 lg:overflow-y-auto",
  xl: "xl:min-h-0 xl:flex-1 xl:overflow-y-auto",
} as const;

export function FormBuilderPage() {
  const { id } = useParams<{ id: string }>();
  const formId = Number(id);
  const valid = Number.isFinite(formId);

  const { resolved: theme } = useTheme();
  const { data: form, isLoading } = useForm(valid ? formId : null);
  // The saved theme as the live embed resolves it (own, else default). Themes
  // aren't edited in the builder, so the saved value is the right one.
  const { data: formTheme } = useResolvedFormTheme(valid ? formId : null);
  const update = useUpdateForm();
  // `forms:read` is enough to reach this page — the route guard asks for no
  // more — so the whole builder degrades to read-only rather than 403ing at
  // Save after the layout has already been reorganised.
  const canEdit = usePermissions().has("forms:write");
  // Whether to warn that a file field has nowhere to put its files. Gated on
  // the permission the endpoint requires, so a forms-only editor doesn't fire
  // a request that can only 403 — they'd get no warning, which is the right
  // trade: storage is an operator's concern, not theirs.
  const canReadStorage = usePermissions().has("storage_config:write");
  const storage = useStorageConfig({ enabled: canReadStorage });

  const [items, setItems] = useState<BuilderElement[] | null>(null);
  const [showPreview, setShowPreview] = useState(true);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  /** What the last paste had to change, so it isn't silent. */
  const [pasteNote, setPasteNote] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const canvasRef = useRef<HTMLDivElement | null>(null);

  const selection = useSelection(items);
  /**
   * The Inspector edits exactly one element, so it only has a subject when
   * exactly one is selected. Deriving it this way is what lets multi-select
   * arrive without touching `Inspector.tsx` at all — with two or more selected
   * it falls to its existing "nothing selected" branch, and the count line
   * below says why.
   */
  const selectedId = selection.ids.length === 1 ? selection.ids[0]! : null;

  // Seed once the form loads. `layout` is always populated by the server —
  // derived from the legacy columns for forms written before it existed.
  useEffect(() => {
    if (form && items === null) setItems(withIds(form.layout));
  }, [form, items]);

  const errors = useMemo(() => (items ? validateLayout(items) : {}), [items]);
  const errorCount = Object.keys(errors).length;
  const hasFileField = (items ?? []).some(
    (it) => it.element.element === "custom" && it.element.config.type === "file",
  );
  const dirty = useMemo(() => {
    if (!form || !items) return false;
    return JSON.stringify(stripIds(items)) !== JSON.stringify(form.layout);
  }, [form, items]);

  const selectedIndex = items?.findIndex((i) => i.id === selectedId) ?? -1;
  const selected = selectedIndex >= 0 ? (items?.[selectedIndex] ?? null) : null;

  const append = (element: FormElement) => appendMany([element]);

  /**
   * Append one or more elements and select the first.
   *
   * A row is two elements — the `row_start`/`row_end` marker pair — so adding
   * one is not a single append. Selecting the opener is what puts the row's
   * settings in the inspector rather than the (settingless) closer.
   */
  const appendMany = (elements: FormElement[]) => {
    const added = elements.map((element) => ({ id: newId(), element }));
    if (added.length === 0) return;
    setItems((prev) => [...(prev ?? []), ...added]);
    // Only the first: a row is a marker pair, and selecting both would leave the
    // Inspector with no single subject and no way to name the row.
    selection.replace([added[0]!.id]);
    setSavedAt(null);
  };

  const patchSelected = (element: FormElement) => {
    setItems((prev) => {
      const before = prev ?? [];
      const current = before.find((i) => i.id === selectedId);
      let next = before.map((i) => (i.id === selectedId ? { ...i, element } : i));
      // A custom field's key is editable, and rules reference keys. Repoint them
      // as it is retyped so a rename doesn't dangle every rule that depends on
      // this field and block save on an error the user can't see the cause of.
      const from = current ? elementKey(current.element) : null;
      const to = elementKey(element);
      if (from && to !== null && from !== to) {
        next = renameRuleReferences(next, from, to);
        // A state picker names its country by key too, so it dangles the same
        // way a rule does.
        next = renameCountryReferences(next, from, to);
      }
      // Retyping a country field to something else strands every state picker
      // that named it, exactly as deleting it would.
      if (from && current && isCountryField(current.element) && !isCountryField(element)) {
        next = stripCountryReferences(next, from);
      }
      return next;
    });
    setSavedAt(null);
  };

  /** Copy one rule onto several elements at once — a branch usually hides a block. */
  const applyRuleToMany = (ids: string[], rule: VisibilityRule) => {
    const targets = new Set(ids);
    setItems((prev) =>
      (prev ?? []).map((i) =>
        targets.has(i.id) ? { ...i, element: withRule(i.element, rule) } : i,
      ),
    );
    setSavedAt(null);
  };

  /**
   * Delete by id. The trash button and a bulk delete are the same operation —
   * `removeMany` pairs up row markers and repairs every rule and country
   * binding that pointed into what went — so there is one path, not two that
   * can drift. The selection prunes itself against the new list.
   */
  const removeIds = (ids: string[]) => {
    if (ids.length === 0) return;
    const drop = new Set(ids);
    setItems((prev) => removeMany(prev ?? [], drop));
    setSavedAt(null);
  };

  /** Elements in document order, for anything acting on the selection. */
  const selectedElements = (): FormElement[] =>
    items ? extractBlock(items, selection.set) : [];

  /** Where a paste lands: after the last selected element, else at the end. */
  const insertAfterSelection = (list: BuilderElement[]): number => {
    const last = selection.ids[selection.ids.length - 1];
    if (!last) return list.length;
    const at = list.findIndex((i) => i.id === last);
    return at === -1 ? list.length : at + 1;
  };

  /** Turn a paste's repairs into one line the author can act on. */
  const noteFor = (outcome: PasteOutcome): string | null => {
    const parts: string[] = [];
    const renames = Object.entries(outcome.renamed);
    if (renames.length > 0) {
      parts.push(
        `renamed ${renames.length === 1 ? "one key" : `${renames.length} keys`} already in use (${renames
          .slice(0, 3)
          .map(([from, to]) => `${from} → ${to}`)
          .join(", ")}${renames.length > 3 ? ", …" : ""})`,
      );
    }
    if (outcome.droppedStandard.length > 0) {
      parts.push(`skipped ${outcome.droppedStandard.join(", ")} — already on this form`);
    }
    if (outcome.repaired > 0) {
      parts.push(
        `cleared ${outcome.repaired === 1 ? "a reference" : `${outcome.repaired} references`} to fields that didn't come along`,
      );
    }
    return parts.length > 0 ? `Pasted, and ${parts.join("; ")}.` : null;
  };

  const applyPaste = (elements: FormElement[], at?: number) => {
    if (!canEdit || !items) return;
    const outcome = prepareForPaste(elements, items, at ?? insertAfterSelection(items));
    if (outcome.refused) {
      setPasteNote(outcome.refused);
      return;
    }
    setItems(outcome.items);
    selection.replace(outcome.pastedIds);
    setPasteNote(noteFor(outcome));
    setSavedAt(null);
  };

  const duplicateSelection = () => {
    if (!canEdit || !items || selection.ids.length === 0) return;
    applyPaste(selectedElements(), insertAfterSelection(items));
  };

  const deleteSelection = () => {
    if (!canEdit || selection.ids.length === 0) return;
    if (selection.ids.length > CONFIRM_DELETE_ABOVE) {
      setConfirmDelete(true);
      return;
    }
    removeIds(selection.ids);
  };

  // Shortcuts. Copy stays available read-only — lifting a block out of a form
  // you can only read is harmless, and useful.
  useBuilderClipboard(
    canvasRef,
    {
      canEdit,
      copy: () => (selection.ids.length > 0 ? selectedElements() : null),
      cut: () => {
        if (selection.ids.length === 0) return null;
        const block = selectedElements();
        removeIds(selection.ids);
        return block;
      },
      paste: (elements) => applyPaste(elements),
      duplicate: duplicateSelection,
      remove: deleteSelection,
      selectAll: selection.selectAll,
      clear: selection.clear,
    },
    dragActive,
  );

  const save = () => {
    if (!canEdit || !items || errorCount > 0) return;
    setSaveError(null);
    setPasteNote(null);
    // Only `layout` goes up — the server derives standard_fields/custom_fields
    // from it, and sending both is rejected.
    update.mutate(
      { id: formId, input: { layout: stripIds(items) } },
      {
        onSuccess: () => setSavedAt(Date.now()),
        onError: (e) => setSaveError(e.message),
      },
    );
  };

  // The schema the preview renders: the live, unsaved layout. The legacy pair
  // is carried through only to satisfy the type — the renderer prefers
  // `layout` whenever it's present.
  const previewSchema: PublicFormDto | null = useMemo(() => {
    if (!form || !items) return null;
    return {
      id: form.id,
      // The same fallback `public_dto_from_model` applies. The admin DTO keeps
      // the two apart, so the preview has to resolve them or it would show the
      // internal name where the live embed shows the display name.
      name: form.display_name || form.name,
      slug: form.slug,
      standard_fields: form.standard_fields as PublicFormDto["standard_fields"],
      custom_fields: [],
      layout: stripIds(items) as PublicFormDto["layout"],
      // Carried through so the preview shows the configured confirmation
      // rather than the built-in default. It's edited in the settings dialog,
      // not here, so it just rides along from the loaded form.
      post_submission_action: form.post_submission_action,
      // Same deal, and note the renderer's field is optional for forward
      // compatibility — omitting it here compiles fine and silently previews
      // the default bar instead of the form's real setting.
      progress_indicator: form.progress_indicator,
      // The server sends this only to forms that need it, to keep the embed
      // script small. The admin has no such budget and the layout here is
      // unsaved anyway, so just always hand the preview the whole table.
      regions: PACKED_SUBDIVISIONS,
      theme: formTheme ?? null,
    };
  }, [form, items, formTheme]);

  // The grid's breakpoint moves with the preview toggle, and the panels have to
  // follow it exactly — see SIDE_PANEL.
  const bp = showPreview ? "xl" : "lg";
  const panel = SIDE_PANEL[bp];
  const panelBody = SIDE_PANEL_BODY[bp];

  if (!valid) return <p className="text-sm text-destructive">Invalid form id.</p>;
  if (isLoading || !form || !items) {
    return (
      <div className="space-y-3">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <Button variant="ghost" size="sm" asChild>
          <Link to="/forms">
            <ArrowLeft className="h-4 w-4 mr-1" />
            Forms
          </Link>
        </Button>
        <div className="flex-1 min-w-0">
          <h1 className="text-xl font-semibold truncate">{form.name}</h1>
          <p className="text-xs text-muted-foreground">
            Drag to reorder. Standard and custom fields can be freely
            interleaved.
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setShowPreview((v) => !v)}
        >
          {showPreview ? (
            <EyeOff className="h-4 w-4 mr-1" />
          ) : (
            <Eye className="h-4 w-4 mr-1" />
          )}
          {showPreview ? "Hide preview" : "Show preview"}
        </Button>
        {canEdit && (
          <Button
            size="sm"
            onClick={save}
            disabled={!dirty || errorCount > 0 || update.isPending}
          >
            {update.isPending ? "Saving…" : "Save layout"}
          </Button>
        )}
      </div>

      {!canEdit && (
        <Alert>
          <AlertDescription>
            Read-only — you can inspect this layout and preview it, but
            changing it needs the{" "}
            <code className="rounded bg-muted px-1 py-0.5 text-xs font-mono">
              forms:write
            </code>{" "}
            permission.
          </AlertDescription>
        </Alert>
      )}

      {/*
        A file field is perfectly legal to save without a provider — storage
        is deployment-wide state a form author may not control, so the server
        doesn't reject it. Warn here instead, where it can be acted on.
      */}
      {hasFileField && canReadStorage && !storage.isPending && !storage.data && (
        <Alert variant="destructive">
          <AlertDescription>
            This form has a file upload field, but no file storage is
            configured — visitors won't be able to attach anything. Set it up
            under <Link to="/settings/storage" className="underline">Settings → File storage</Link>.
          </AlertDescription>
        </Alert>
      )}

      {errorCount > 0 && (
        <Alert variant="destructive">
          <AlertDescription>
            {errorCount === 1
              ? "One element needs attention before you can save."
              : `${errorCount} elements need attention before you can save.`}
          </AlertDescription>
        </Alert>
      )}
      {saveError && (
        <Alert variant="destructive">
          <AlertDescription>{saveError}</AlertDescription>
        </Alert>
      )}
      {pasteNote && (
        <Alert>
          <AlertDescription className="flex items-start justify-between gap-3">
            <span>{pasteNote}</span>
            <button
              type="button"
              className="text-xs underline shrink-0"
              onClick={() => setPasteNote(null)}
            >
              Dismiss
            </button>
          </AlertDescription>
        </Alert>
      )}
      {savedAt && !dirty && (
        <Alert>
          <AlertDescription>Layout saved.</AlertDescription>
        </Alert>
      )}

      <div
        className={
          showPreview
            ? "grid gap-4 xl:grid-cols-[minmax(0,13rem)_minmax(0,1fr)_minmax(0,17rem)_minmax(0,20rem)]"
            : "grid gap-4 lg:grid-cols-[minmax(0,13rem)_minmax(0,1fr)_minmax(0,17rem)]"
        }
      >
        <Card className={panel}>
          <CardHeader className="pb-2 shrink-0">
            <CardTitle className="text-sm">Add</CardTitle>
          </CardHeader>
          <CardContent className={panelBody}>
            <fieldset disabled={!canEdit} className="min-w-0 border-0 p-0 m-0">
              <Palette
                usedStandard={usedStandardKeys(items)}
                onAddStandard={(key) => append(newStandardElement(key))}
                onAddCustom={(type: CustomTypeName) =>
                  append(newCustomElement(type, items.length, items))
                }
                onAddDecoration={(kind) => append(newDecorationElement(kind))}
                onAddRow={() => appendMany(newRowElements())}
              />
            </fieldset>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Fields</CardTitle>
          </CardHeader>
          <CardContent>
            {/* The ref scopes Delete and select-all to the canvas: an unscoped
                Backspace deleting fields from wherever focus happened to be
                would be alarming. */}
            <div ref={canvasRef}>
              <SelectionToolbar
                count={selection.ids.length}
                canEdit={canEdit}
                onCopy={() => {
                  const block = selectedElements();
                  if (block.length === 0) return;
                  const payload = serializeElements(block);
                  writeLocalClipboard(payload);
                  void navigator.clipboard?.writeText(payload).catch(() => {
                    // Non-secure context: the localStorage mirror above still
                    // carries it, which is enough for a paste in this browser.
                  });
                }}
                onCut={() => {
                  const block = selectedElements();
                  if (block.length === 0) return;
                  const payload = serializeElements(block);
                  writeLocalClipboard(payload);
                  void navigator.clipboard?.writeText(payload).catch(() => {});
                  removeIds(selection.ids);
                }}
                onDuplicate={duplicateSelection}
                onDelete={deleteSelection}
                onPaste={(elements) => applyPaste(elements)}
                onClear={selection.clear}
              />
              <Canvas
                items={items}
                selectedIds={selection.set}
                errors={errors}
                onSelect={selection.select}
                onRemove={(rid) => removeIds([rid])}
                onReorder={(next) => {
                  setItems(next);
                  setSavedAt(null);
                }}
                onDragActiveChange={setDragActive}
                readOnly={!canEdit}
              />
            </div>
          </CardContent>
        </Card>

        <Card className={panel}>
          <CardHeader className="pb-2 shrink-0">
            <CardTitle className="text-sm">Settings</CardTitle>
          </CardHeader>
          <CardContent className={panelBody}>
            {/* The Inspector edits one element. Say so, rather than letting it
                show its bare "select an element" line next to a live
                multi-selection. */}
            {selection.ids.length > 1 && (
              <p className="mb-2 text-sm text-muted-foreground">
                {selection.ids.length} elements selected. Settings apply to one
                element at a time — use the buttons above the list to copy,
                duplicate or delete the whole block.
              </p>
            )}
            <fieldset disabled={!canEdit} className="min-w-0 border-0 p-0 m-0">
              <Inspector
                item={selected}
                onChange={patchSelected}
                inRow={isInsideRow(items, selectedIndex)}
                candidates={controllerCandidates(items, selectedIndex)}
                countryCandidates={countryFieldCandidates(items, selectedIndex)}
                // Guarded: with nothing selected `selectedIndex` is -1 and a
                // bare `slice(0)` would offer the whole form as rule targets,
                // including elements *earlier* than the rule's owner.
                ruleTargets={selectedIndex >= 0 ? items.slice(selectedIndex + 1) : []}
                onApplyRuleToMany={applyRuleToMany}
              />
            </fieldset>
          </CardContent>
        </Card>

        {/* There is no undo in this builder, so a bulk delete asks first. */}
        <ConfirmDialog
          open={confirmDelete}
          onOpenChange={setConfirmDelete}
          title={`Delete ${selection.ids.length} elements?`}
          description="This can't be undone, though nothing is saved until you press Save layout."
          confirmLabel="Delete"
          onConfirm={() => {
            removeIds(selection.ids);
            setConfirmDelete(false);
          }}
        />

        {showPreview && previewSchema && (
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Preview</CardTitle>
            </CardHeader>
            <CardContent>
              {/*
                Rendered through the same ShadowForm the embed uses, fed the
                unsaved layout. previewMode disables submit so experimenting
                here can't create real submissions or fire deliveries.
              */}
              <ShadowForm
                formId={String(form.id)}
                apiUrl={api.baseUrl}
                schema={previewSchema}
                previewMode
                theme={theme}
              />
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  );
}

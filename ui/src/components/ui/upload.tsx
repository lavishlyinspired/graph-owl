import * as React from "react";
import { cn } from "../../lib/utils";
import { Button } from "./button";
import { UploadOutlined } from "./icons";

const UPLOAD_LABEL = "Upload";
const UPLOADING_LABEL = "uploading…";

export interface UploadFile {
  readonly uid: string;
  readonly name: string;
  readonly status?: "uploading" | "done" | "error" | "removed";
  readonly url?: string;
  readonly originFileObj?: File;
  readonly response?: unknown;
}

interface UploadProps {
  readonly accept?: string;
  readonly multiple?: boolean;
  readonly beforeUpload?: (file: File, fileList: File[]) => boolean | Promise<boolean>;
  readonly customRequest?: (options: { file: File; onSuccess?: () => void; onError?: () => void }) => void;
  readonly onChange?: (info: { file: UploadFile; fileList: UploadFile[] }) => void;
  readonly fileList?: UploadFile[];
  readonly children?: React.ReactNode;
  readonly className?: string;
  readonly showUploadList?: boolean;
  readonly directory?: boolean;
  readonly disabled?: boolean;
  readonly maxCount?: number;
}

const UploadInternal = React.forwardRef<HTMLInputElement, UploadProps>(
  (
    {
      accept,
      multiple,
      beforeUpload,
      customRequest,
      onChange,
      fileList,
      children,
      className,
      showUploadList = true,
    },
    ref,
  ) => {
    const inputRef = React.useRef<HTMLInputElement>(null);
    const [internalList, setInternalList] = React.useState<UploadFile[]>([]);
    const list = fileList ?? internalList;

    const handleFiles = async (files: FileList | null) => {
      if (!files) return;
      const fileArray = Array.from(files);
      for (const file of fileArray) {
        let ok = true;
        if (beforeUpload) {
          const result = beforeUpload(file, fileArray);
          if (result instanceof Promise) ok = await result;
          else ok = result;
        }
        if (!ok) continue;

        const item: UploadFile = {
          uid: `${Date.now()}-${Math.random()}`,
          name: file.name,
          status: "uploading",
          originFileObj: file,
        };
        const nextList = [...list, item];
        if (fileList === undefined) setInternalList(nextList);
        onChange?.({ file: item, fileList: nextList });

        if (customRequest) {
          customRequest({
            file,
            onSuccess: () => {
              const done = { ...item, status: "done" as const };
              const final = nextList.map((f) => (f.uid === item.uid ? done : f));
              if (fileList === undefined) setInternalList(final);
              onChange?.({ file: done, fileList: final });
            },
            onError: () => {
              const err = { ...item, status: "error" as const };
              const final = nextList.map((f) => (f.uid === item.uid ? err : f));
              if (fileList === undefined) setInternalList(final);
              onChange?.({ file: err, fileList: final });
            },
          });
        } else {
          const done = { ...item, status: "done" as const };
          const final = nextList.map((f) => (f.uid === item.uid ? done : f));
          if (fileList === undefined) setInternalList(final);
          onChange?.({ file: done, fileList: final });
        }
      }
    };

    return (
      <div className={cn("flex flex-col gap-2", className)}>
        <Button
          type="button"
          variant="outline"
          onClick={() => inputRef.current?.click()}
        >
          {children ?? (
            <>
              <UploadOutlined size={16} /> {UPLOAD_LABEL}
            </>
          )}
        </Button>
        <input
          ref={(node) => {
            inputRef.current = node;
            if (typeof ref === "function") ref(node);
            else if (ref) (ref as React.MutableRefObject<HTMLInputElement | null>).current = node;
          }}
          type="file"
          accept={accept}
          multiple={multiple}
          className="hidden"
          onChange={(e) => {
            handleFiles(e.target.files);
            e.target.value = "";
          }}
        />
        {showUploadList ? (
          <ul className="flex flex-col gap-1 text-sm">
            {list.map((file) => (
              <li
                key={file.uid}
                className={cn(
                  "flex items-center gap-2",
                  file.status === "error" && "text-red-600",
                  file.status === "done" && "text-green-700",
                )}
              >
                <span className="truncate">{file.name}</span>
                {file.status === "uploading" ? (
                  <span className="text-xs text-[var(--gowl-text-subtle)]">{UPLOADING_LABEL}</span>
                ) : null}
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    );
  },
);
UploadInternal.displayName = "Upload";

interface DraggerProps extends UploadProps {
  readonly style?: React.CSSProperties;
  readonly children?: React.ReactNode;
}

export const UploadDragger = React.forwardRef<HTMLDivElement, DraggerProps>(
  (
    { className, style, accept, multiple, beforeUpload, customRequest, onChange, fileList, disabled, maxCount, children },
    ref,
  ) => {
    const inputRef = React.useRef<HTMLInputElement>(null);
    const [internalList, setInternalList] = React.useState<UploadFile[]>([]);
    const list = fileList ?? internalList;
    const [dragOver, setDragOver] = React.useState(false);

    const handleFiles = async (files: FileList | null) => {
      if (disabled || !files) return;
      const remaining = maxCount !== undefined ? Math.max(0, maxCount - list.length) : undefined;
      const fileArray = remaining !== undefined ? Array.from(files).slice(0, remaining) : Array.from(files);
      for (const file of fileArray) {
        let ok = true;
        if (beforeUpload) {
          const result = beforeUpload(file, fileArray);
          if (result instanceof Promise) ok = await result;
          else ok = result;
        }
        if (!ok) continue;

        const item: UploadFile = {
          uid: `${Date.now()}-${Math.random()}`,
          name: file.name,
          status: "uploading",
          originFileObj: file,
        };
        const nextList = [...list, item];
        if (fileList === undefined) setInternalList(nextList);
        onChange?.({ file: item, fileList: nextList });

        if (customRequest) {
          customRequest({
            file,
            onSuccess: () => {
              const done = { ...item, status: "done" as const };
              const final = nextList.map((f) => (f.uid === item.uid ? done : f));
              if (fileList === undefined) setInternalList(final);
              onChange?.({ file: done, fileList: final });
            },
            onError: () => {
              const err = { ...item, status: "error" as const };
              const final = nextList.map((f) => (f.uid === item.uid ? err : f));
              if (fileList === undefined) setInternalList(final);
              onChange?.({ file: err, fileList: final });
            },
          });
        } else {
          const done = { ...item, status: "done" as const };
          const final = nextList.map((f) => (f.uid === item.uid ? done : f));
          if (fileList === undefined) setInternalList(final);
          onChange?.({ file: done, fileList: final });
        }
      }
    };

    return (
      <div
        ref={ref}
        className={cn(
          "flex cursor-pointer flex-col items-center justify-center rounded-[var(--gowl-radius-card)] border border-dashed border-[var(--gowl-border)] bg-[var(--gowl-fill-subtle)] p-6 text-center transition-colors hover:border-[var(--gowl-primary)]",
          dragOver && "border-[var(--gowl-primary)] bg-[var(--gowl-fill)]",
          disabled && "pointer-events-none cursor-not-allowed opacity-50",
          className,
        )}
        style={style}
        onClick={() => inputRef.current?.click()}
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
          handleFiles(e.dataTransfer.files);
        }}
      >
        <input
          ref={inputRef}
          type="file"
          accept={accept}
          multiple={multiple}
          disabled={disabled}
          className="hidden"
          onChange={(e) => {
            handleFiles(e.target.files);
            e.target.value = "";
          }}
        />
        {children}
      </div>
    );
  },
);
UploadDragger.displayName = "UploadDragger";

export const Upload = Object.assign(UploadInternal, { Dragger: UploadDragger }) as typeof UploadInternal & {
  Dragger: typeof UploadDragger;
};

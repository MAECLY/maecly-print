interface MPDividerProps {
  vertical?: boolean;
}

export function MPDivider({ vertical }: MPDividerProps) {
  if (vertical) {
    return (
      <span
        style={{
          width: 1,
          alignSelf: 'stretch',
          background: 'var(--mp-border)',
        }}
      />
    );
  }
  return (
    <div style={{ height: 1, background: 'var(--mp-border)', width: '100%' }} />
  );
}

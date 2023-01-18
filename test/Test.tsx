
const button = classed('button', 'w-full, relative');

export const Test = () => {
  const classes = clsx("w-full relative");
  return (
    <div className="w-full relative">
      <div className={clsx("w-full relative")} />
      <div innerClassName="w-full relative" />
    </div>
  );
};

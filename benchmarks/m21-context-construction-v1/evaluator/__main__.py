import argparse
from pathlib import Path
from .canonical import load, write
from .harness import TreatmentSource, dry_run
from .oracle import derive
from .selector import enumerate_candidates, select

parser=argparse.ArgumentParser(); sub=parser.add_subparsers(dest="command",required=True)
p=sub.add_parser("oracle"); p.add_argument("repository"); p.add_argument("repository_id"); p.add_argument("base_oid"); p.add_argument("fix_oid"); p.add_argument("output")
p=sub.add_parser("dry-run"); p.add_argument("task"); p.add_argument("output"); p.add_argument("--arm",choices=["A","B"],default="A"); p.add_argument("--repository"); p.add_argument("--repository-id"); p.add_argument("--base-oid")
p=sub.add_parser("enumerate"); p.add_argument("repository",choices=["reviewgraphen","fsl","casegraphen"]); p.add_argument("output")
p=sub.add_parser("select"); p.add_argument("repository",choices=["reviewgraphen","fsl","casegraphen"]); p.add_argument("output")
args=parser.parse_args()
if args.command=="oracle": write(Path(args.output), derive(args.repository,args.repository_id,args.base_oid,args.fix_oid))
elif args.command=="dry-run":
    if not all((args.repository,args.repository_id,args.base_oid)): parser.error("arms A/B require --repository, --repository-id, and --base-oid")
    source=TreatmentSource(Path(args.repository),args.repository_id,args.base_oid)
    write(Path(args.output), dry_run(load(Path(args.task)),source,arm=args.arm).record())
elif args.command=="enumerate": write(Path(args.output),enumerate_candidates(args.repository))
else: write(Path(args.output),{"schema":"m21.selected_tasks.v1","tasks":select(args.repository)})

#!/usr/bin/env python3
"""Generate a reproducible, non-private Alina Chronexa-subset benchmark."""
import argparse
import json

parser = argparse.ArgumentParser()
parser.add_argument('--classes', type=int, default=30)
parser.add_argument('--occurrences', type=int, default=4)
args = parser.parse_args()
if args.classes < 1 or not 1 <= args.occurrences <= 30:
    parser.error('classes must be positive and occurrences must be 1..30')
count = args.classes
school = {
    'daysPerWeek': 5, 'periodsPerDay': 6,
    'teachers': [{'id': f't{i}', 'name': f'T{i}'} for i in range(count)],
    'classes': [{'id': f'c{i}', 'name': f'C{i}', 'size': 20} for i in range(count)],
    'classrooms': [], 'subjects': [{'id': 's', 'name': 'S'}],
    'lessons': [{'id': f'l{i}', 'subjectId': 's', 'teacherIds': [f't{i}'],
                 'classIds': [f'c{i}'], 'periodsPerWeek': args.occurrences}
                for i in range(count)],
}
print(json.dumps(school, sort_keys=True, separators=(',', ':')))

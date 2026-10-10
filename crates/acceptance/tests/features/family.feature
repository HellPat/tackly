Feature: A family of three shares its tasks
  Patrick is the head of the family. Mona and Mara join with an invitation
  (QR code or link). Everyone sees the same tasks, live.

  Scenario: Patrick creates a family and connects Mona by QR code and Mara by link
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    When Patrick creates the family "The Smiths"
    And Patrick invites Mona by QR code
    And Patrick invites Mara by link
    Then Patrick, Mona and Mara see the members Patrick, Mona and Mara

  Scenario: New tasks land in Other and everyone sees them
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick adds the tasks "Do the dishes" and "Take out the trash"
    And Mona shows All
    And Mara shows All
    Then Mona and Mara see the tasks "Do the dishes" and "Take out the trash"

  Scenario: The others watch Mona work, and the task is gone once she finishes
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Water the plants"
    And Mona shows All
    And Patrick shows All
    When Mona starts "Water the plants"
    Then Patrick sees that Mona is working on "Water the plants"
    And Patrick cannot tick "Water the plants" off
    When Mona finishes "Water the plants"
    Then Patrick and Mona no longer see "Water the plants"

  Scenario: Ticking a task off, and taking it back with Undo
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Feed the cat"
    And Patrick shows All
    And Mara shows All
    When Mara ticks "Feed the cat" off
    Then Patrick no longer sees "Feed the cat"
    When Mara undoes "Feed the cat done"
    Then Patrick and Mara see the task "Feed the cat"

  Scenario: Patrick gives a task to Mona; it is in her Mine
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Pick up the parcel"
    And Patrick shows All
    When Patrick gives "Pick up the parcel" to Mona
    Then Patrick sees that Mona has "Pick up the parcel"
    When Mona shows Mine
    Then Mona sees the task "Pick up the parcel"
    And Mona sees 1 in Mine

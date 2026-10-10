Feature: A family of three shares a task list
  Patrick, Mona and Mara each use their own phone. Everything is done the way a
  person does it, in the real app; the sync server is real too.

  Background:
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly

  Scenario: Patrick creates a family and connects Mona by QR code and Mara by link
    When Patrick creates the family "The Smiths"
    And Patrick invites Mona by QR code
    And Patrick invites Mara by link
    Then Patrick, Mona and Mara see the members Patrick, Mona and Mara with Patrick as head of the family

  Scenario: The task list syncs and the others watch Mona work on it live
    Given Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick adds the tasks "Do the dishes" and "Take out the trash"
    Then Mona and Mara see the tasks "Do the dishes" and "Take out the trash"
    When Mona starts "Do the dishes"
    Then Patrick and Mara see "Do the dishes" in progress by Mona
    When Mona finishes "Do the dishes" with the note "Dishwasher was full"
    Then Patrick, Mona and Mara see "Do the dishes" done by Mona with a duration, the note "Dishwasher was full" and a location

  Scenario: Mara finishes a task Mona started, and Patrick reopens it
    Given Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Water the plants"
    When Mona starts "Water the plants"
    And Mara finishes "Water the plants"
    Then Patrick and Mona see "Water the plants" done by Mara
    When Patrick reopens "Water the plants"
    Then Mona and Mara see "Water the plants" open again
